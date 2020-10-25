use std::{
    convert::{Infallible, TryInto},
    fmt::Display,
    ptr::null_mut,
};

use utfx::{U16CStr, U16CString};
use winapi::shared::minwindef::HKEY;
use winapi::um::winreg::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteTreeW, RegOpenCurrentUser, RegOpenKeyExW,
    RegSaveKeyExW,
};

use crate::iter;
use crate::sec::Security;
use crate::{value, Hive};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Provided path not found: {0:?}")]
    NotFound(String, #[source] std::io::Error),

    #[error("Permission denied for given path: {0:?}")]
    PermissionDenied(String, #[source] std::io::Error),

    #[error("Invalid null found in provided path")]
    InvalidNul(#[from] utfx::NulError<u16>),

    #[error("An unknown IO error occurred for given path: {0:?}")]
    Unknown(String, #[source] std::io::Error),
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unsafe { std::hint::unreachable_unchecked() }
    }
}

/// The safe representation of a Windows registry key.
#[derive(Debug)]
pub struct RegKey {
    pub(crate) hive: Hive,
    pub(crate) handle: HKEY,
    pub(crate) path: U16CString,
}

impl Display for RegKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.hive)?;
        let path = self.path.to_string_lossy();

        if path != "" {
            f.write_str(r"\")?;
            f.write_str(&path)?;
        }

        Ok(())
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // No point checking the return value here.
        unsafe { RegCloseKey(self.handle) };
    }
}

impl RegKey {
    #[inline]
    pub fn open<P>(&self, path: P, sec: Security) -> Result<RegKey, Error>
    where
        P: TryInto<U16CString>,
        P::Error: Into<Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        open_hkey(self.handle, &path, sec).map(|handle| RegKey {
            hive: self.hive,
            handle,
            path,
        })
    }

    #[inline]
    pub fn write<P>(&self, file_path: P) -> Result<(), Error>
    where
        P: TryInto<U16CString>,
        P::Error: Into<Error>,
    {
        let path = file_path.try_into().map_err(Into::into)?;
        save_hkey(self.handle, &path)
    }

    #[inline]
    pub fn create<P>(&self, path: P, sec: Security) -> Result<RegKey, Error>
    where
        P: TryInto<U16CString>,
        P::Error: Into<Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        create_hkey(self.handle, &path, sec).map(|handle| RegKey {
            hive: self.hive,
            handle,
            path,
        })
    }

    #[inline]
    pub fn delete<P>(&self, path: P, is_recursive: bool) -> Result<(), Error>
    where
        P: TryInto<U16CString>,
        P::Error: Into<Error>,
    {
        let path = path.try_into().map_err(Into::into)?;
        delete_hkey(self.handle, path, is_recursive)
    }

    #[inline]
    pub fn delete_self(self, is_recursive: bool) -> Result<(), Error> {
        delete_hkey(self.handle, U16CString::default(), is_recursive)
    }

    #[inline]
    pub fn value<S>(&self, value_name: S) -> Result<value::Data, value::Error>
    where
        S: TryInto<U16CString>,
        S::Error: Into<value::Error>,
    {
        value::query_value(self.handle, value_name)
    }

    #[inline]
    pub fn delete_value<S>(&self, value_name: S) -> Result<(), value::Error>
    where
        S: TryInto<U16CString>,
        S::Error: Into<value::Error>,
    {
        value::delete_value(self.handle, value_name)
    }

    #[inline]
    pub fn set_value<S>(&self, value_name: S, data: &value::Data) -> Result<(), value::Error>
    where
        S: TryInto<U16CString>,
        S::Error: Into<value::Error>,
    {
        value::set_value(self.handle, value_name, data)
    }

    #[inline]
    pub fn keys(&self) -> iter::Keys<'_> {
        match iter::Keys::new(self) {
            Ok(v) => v,
            Err(e) => unreachable!(e),
        }
    }

    #[inline]
    pub fn values(&self) -> iter::Values<'_> {
        match iter::Values::new(self) {
            Ok(v) => v,
            Err(e) => unreachable!(e),
        }
    }

    pub fn open_current_user(sec: Security) -> Result<RegKey, Error> {
        let mut hkey = null_mut();

        let result = unsafe { RegOpenCurrentUser(sec.bits(), &mut hkey) };

        if result == 0 {
            // TODO: use NT API to query path
            return Ok(RegKey {
                hive: Hive::CurrentUser,
                handle: hkey,
                path: "".try_into().unwrap(),
            });
        }

        let io_error = std::io::Error::from_raw_os_error(result);
        let path = "<current user>".to_string();
        match io_error.kind() {
            std::io::ErrorKind::NotFound => Err(Error::NotFound(path, io_error)),
            std::io::ErrorKind::PermissionDenied => Err(Error::PermissionDenied(path, io_error)),
            _ => Err(Error::Unknown(path, io_error)),
        }
    }
}

#[inline]
pub(crate) fn open_hkey<'a, P>(base: HKEY, path: P, sec: Security) -> Result<HKEY, Error>
where
    P: AsRef<U16CStr>,
{
    let path = path.as_ref();
    let mut hkey = std::ptr::null_mut();
    let result = unsafe { RegOpenKeyExW(base, path.as_ptr(), 0, sec.bits(), &mut hkey) };

    if result == 0 {
        return Ok(hkey);
    }

    let io_error = std::io::Error::from_raw_os_error(result);
    let path = path.to_string().unwrap_or_else(|_| "<unknown>".into());
    match io_error.kind() {
        std::io::ErrorKind::NotFound => Err(Error::NotFound(path, io_error)),
        std::io::ErrorKind::PermissionDenied => Err(Error::PermissionDenied(path, io_error)),
        _ => Err(Error::Unknown(path, io_error)),
    }
}

#[inline]
pub(crate) fn save_hkey<'a, P>(hkey: HKEY, path: P) -> Result<(), Error>
where
    P: AsRef<U16CStr>,
{
    let path = path.as_ref();
    let result = unsafe { RegSaveKeyExW(hkey, path.as_ptr(), std::ptr::null_mut(), 4) };

    if result == 0 {
        return Ok(());
    }

    let io_error = std::io::Error::from_raw_os_error(result);
    let path = path.to_string().unwrap_or_else(|_| "<unknown>".into());
    match io_error.kind() {
        std::io::ErrorKind::NotFound => Err(Error::NotFound(path, io_error)),
        std::io::ErrorKind::PermissionDenied => Err(Error::PermissionDenied(path, io_error)),
        _ => Err(Error::Unknown(path, io_error)),
    }
}

#[inline]
pub(crate) fn delete_hkey<P>(base: HKEY, path: P, is_recursive: bool) -> Result<(), Error>
where
    P: AsRef<U16CStr>,
{
    let path = path.as_ref();

    let result = if is_recursive {
        unsafe { RegDeleteTreeW(base, path.as_ptr()) }
    } else {
        unsafe { RegDeleteKeyW(base, path.as_ptr()) }
    };

    if result == 0 {
        return Ok(());
    }

    let io_error = std::io::Error::from_raw_os_error(result);
    let path = path.to_string().unwrap_or_else(|_| "<unknown>".into());
    match io_error.kind() {
        std::io::ErrorKind::NotFound => Err(Error::NotFound(path, io_error)),
        std::io::ErrorKind::PermissionDenied => Err(Error::PermissionDenied(path, io_error)),
        _ => Err(Error::Unknown(path, io_error)),
    }
}

#[inline]
pub(crate) fn create_hkey<P>(base: HKEY, path: P, sec: Security) -> Result<HKEY, Error>
where
    P: AsRef<U16CStr>,
{
    let path = path.as_ref();
    let mut hkey = std::ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            base,
            path.as_ptr(),
            0,
            std::ptr::null_mut(),
            0,
            sec.bits(),
            std::ptr::null_mut(),
            &mut hkey,
            std::ptr::null_mut(),
        )
    };

    if result == 0 {
        return Ok(hkey);
    }

    let io_error = std::io::Error::from_raw_os_error(result);
    let path = path.to_string().unwrap_or_else(|_| "<unknown>".into());
    match io_error.kind() {
        std::io::ErrorKind::NotFound => Err(Error::NotFound(path, io_error)),
        std::io::ErrorKind::PermissionDenied => Err(Error::PermissionDenied(path, io_error)),
        _ => Err(Error::Unknown(path, io_error)),
    }
}

#[cfg(test)]
mod tests {
    use crate::Hive;

    #[test]
    fn test_paths() {
        let key = Hive::CurrentUser
            .open("SOFTWARE\\Microsoft", crate::Security::Read)
            .unwrap();
        assert_eq!(key.to_string(), "HKEY_CURRENT_USER\\SOFTWARE\\Microsoft")
    }
}
