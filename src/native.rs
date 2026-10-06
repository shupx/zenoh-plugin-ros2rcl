use anyhow::{anyhow, ensure, Result};
use rclrs::{DynamicMessage, DynamicMessageMetadata, MessageTypeName};
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
};

unsafe extern "C" {
    fn bridge_error() -> *const c_char;
    fn bridge_reset_error();
    fn bridge_context_new(domain: usize, name: *const c_char) -> *mut c_void;
    fn bridge_context_free(ctx: *mut c_void);
    fn bridge_codec_new(
        package: *const c_char,
        ns: *const c_char,
        name: *const c_char,
    ) -> *mut c_void;
    fn bridge_codec_free(codec: *mut c_void);
    fn bridge_serialize(
        codec: *mut c_void,
        msg: *const c_void,
        out: *mut *mut u8,
        size: *mut usize,
    ) -> c_int;
    fn bridge_deserialize(
        codec: *mut c_void,
        data: *const u8,
        size: usize,
        msg: *mut c_void,
    ) -> c_int;
    fn bridge_free(p: *mut c_void);
    fn bridge_endpoint_new(
        ctx: *mut c_void,
        package: *const c_char,
        ty: *const c_char,
        name: *const c_char,
        server: c_int,
        depth: usize,
    ) -> *mut c_void;
    fn bridge_endpoint_free(endpoint: *mut c_void);
    fn bridge_take_request(
        endpoint: *mut c_void,
        msg: *mut c_void,
        header: *mut *mut c_void,
    ) -> c_int;
    fn bridge_send_response(endpoint: *mut c_void, header: *mut c_void, msg: *mut c_void) -> c_int;
    fn bridge_send_request(endpoint: *mut c_void, msg: *mut c_void, sequence: *mut i64) -> c_int;
    fn bridge_take_response(endpoint: *mut c_void, msg: *mut c_void, sequence: *mut i64) -> c_int;
}
fn error(operation: &str) -> anyhow::Error {
    // RCL error storage is thread-local; copy before resetting it.
    unsafe {
        let message = CStr::from_ptr(bridge_error())
            .to_string_lossy()
            .into_owned();
        bridge_reset_error();
        anyhow!("{operation}: {message} (verify sourced ROS type support libraries)")
    }
}
fn check(code: c_int, operation: &str) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(error(operation))
    }
}

pub struct NativeContext(NonNull<c_void>);
impl NativeContext {
    pub fn new(domain: usize, name: &str) -> Result<Rc<Self>> {
        let name = CString::new(name)?;
        let ptr = unsafe { bridge_context_new(domain, name.as_ptr()) };
        Ok(Rc::new(Self(
            NonNull::new(ptr).ok_or_else(|| error("create RCL context"))?,
        )))
    }
}
impl Drop for NativeContext {
    fn drop(&mut self) {
        unsafe { bridge_context_free(self.0.as_ptr()) }
    }
}

pub struct Codec {
    ptr: NonNull<c_void>,
    metadata: DynamicMessageMetadata,
    ty: String,
}
// Type support is immutable; each operation receives independent message storage.
unsafe impl Send for Codec {}
unsafe impl Sync for Codec {}
impl Codec {
    pub fn new(ty: &str) -> Result<Arc<Self>> {
        let p: Vec<_> = ty.split('/').collect();
        ensure!(p.len() == 3, "invalid message type");
        let message_type = MessageTypeName {
            package_name: p[0].into(),
            type_name: p[2].into(),
        };
        let metadata = if p[1] == "srv" {
            DynamicMessageMetadata::new_service_message(message_type)?
        } else {
            DynamicMessageMetadata::new(message_type)?
        };
        let package = CString::new(p[0])?;
        let ns = CString::new(p[1])?;
        let name = CString::new(p[2])?;
        let ptr = unsafe { bridge_codec_new(package.as_ptr(), ns.as_ptr(), name.as_ptr()) };
        Ok(Arc::new(Self {
            ptr: NonNull::new(ptr).ok_or_else(|| error("load message codec"))?,
            metadata,
            ty: ty.into(),
        }))
    }
    pub fn message(&self) -> Result<DynamicMessage> {
        Ok(self.metadata.create()?)
    }
    pub fn encode(&self, message: &DynamicMessage) -> Result<Vec<u8>> {
        let mut data = std::ptr::null_mut();
        let mut size = 0;
        check(
            unsafe {
                bridge_serialize(
                    self.ptr.as_ptr(),
                    message.native_ptr(),
                    &mut data,
                    &mut size,
                )
            },
            "serialize",
        )?;
        // Version and type tag prevent decoding a key with mismatched ROS types.
        let mut out = b"R2R\x01".to_vec();
        out.extend_from_slice(&(self.ty.len() as u32).to_le_bytes());
        out.extend_from_slice(self.ty.as_bytes());
        unsafe {
            out.extend_from_slice(std::slice::from_raw_parts(data, size));
            bridge_free(data.cast());
        }
        Ok(out)
    }
    pub fn decode(&self, data: &[u8]) -> Result<DynamicMessage> {
        ensure!(
            data.len() >= 8 && &data[..4] == b"R2R\x01",
            "invalid ROS2RCL payload version"
        );
        let n = u32::from_le_bytes(data[4..8].try_into()?) as usize;
        ensure!(
            n <= data.len() - 8 && &data[8..8 + n] == self.ty.as_bytes(),
            "ROS payload type mismatch"
        );
        let cdr = &data[8 + n..];
        ensure!(cdr.len() >= 4, "truncated CDR");
        let mut msg = self.message()?;
        check(
            unsafe {
                bridge_deserialize(
                    self.ptr.as_ptr(),
                    cdr.as_ptr(),
                    cdr.len(),
                    msg.native_mut_ptr(),
                )
            },
            "deserialize",
        )?;
        Ok(msg)
    }
}
impl Drop for Codec {
    fn drop(&mut self) {
        unsafe { bridge_codec_free(self.ptr.as_ptr()) }
    }
}

pub struct Header(NonNull<c_void>);
impl Drop for Header {
    fn drop(&mut self) {
        unsafe { bridge_free(self.0.as_ptr()) }
    }
}
pub struct Endpoint {
    ptr: NonNull<c_void>,
    _context: Rc<NativeContext>,
}
impl Endpoint {
    pub fn new(
        context: Rc<NativeContext>,
        ty: &str,
        name: &str,
        server: bool,
        depth: usize,
    ) -> Result<Self> {
        let (package, kind) = crate::config::type_parts(ty, "srv")?;
        let package = CString::new(package)?;
        let kind = CString::new(kind)?;
        let name = CString::new(name)?;
        let ptr = unsafe {
            bridge_endpoint_new(
                context.0.as_ptr(),
                package.as_ptr(),
                kind.as_ptr(),
                name.as_ptr(),
                server.into(),
                depth,
            )
        };
        Ok(Self {
            ptr: NonNull::new(ptr).ok_or_else(|| error("create service endpoint"))?,
            _context: context,
        })
    }
    /// `message` must point to initialized storage of this endpoint's request type.
    pub unsafe fn take_request(&self, message: *mut c_void) -> Result<Option<Header>> {
        let mut header = std::ptr::null_mut();
        match bridge_take_request(self.ptr.as_ptr(), message, &mut header) {
            0 => Ok(Some(Header(NonNull::new(header).expect("native header")))),
            1 => Ok(None),
            _ => Err(error("take request")),
        }
    }
    /// `message` must be initialized storage of this endpoint's response type.
    pub unsafe fn respond(&self, header: &Header, message: *mut c_void) -> Result<()> {
        check(
            bridge_send_response(self.ptr.as_ptr(), header.0.as_ptr(), message),
            "send response",
        )
    }
    pub fn request(&self, message: &mut DynamicMessage) -> Result<i64> {
        let mut seq = 0;
        check(
            unsafe { bridge_send_request(self.ptr.as_ptr(), message.native_mut_ptr(), &mut seq) },
            "send request",
        )?;
        Ok(seq)
    }
    pub fn take_response(&self, message: &mut DynamicMessage) -> Result<Option<i64>> {
        let mut seq = 0;
        match unsafe { bridge_take_response(self.ptr.as_ptr(), message.native_mut_ptr(), &mut seq) }
        {
            0 => Ok(Some(seq)),
            1 => Ok(None),
            _ => Err(error("take response")),
        }
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        unsafe { bridge_endpoint_free(self.ptr.as_ptr()) }
    }
}
pub fn service_codecs(ty: &str) -> Result<(Arc<Codec>, Arc<Codec>)> {
    Ok((
        Codec::new(&format!("{ty}_Request"))?,
        Codec::new(&format!("{ty}_Response"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rclrs::{SimpleValue, SimpleValueMut, Value, ValueMut};

    #[test]
    fn dynamic_message_cdr_and_invalid_payloads() {
        let codec = Codec::new("std_msgs/msg/String").unwrap();
        let mut message = codec.message().unwrap();
        if let Some(ValueMut::Simple(SimpleValueMut::String(s))) = message.get_mut("data") {
            *s = "round trip".into();
        } else {
            panic!("missing string field");
        }
        let data = codec.encode(&message).unwrap();
        let result = codec.decode(&data).unwrap();
        match result.get("data") {
            Some(Value::Simple(SimpleValue::String(s))) => assert_eq!(s.to_string(), "round trip"),
            _ => panic!("missing string field"),
        }
        assert!(codec.decode(b"garbage").is_err());
        let mut truncated = b"R2R\x01".to_vec();
        truncated.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(codec.decode(&truncated).is_err());
        let other = Codec::new("std_msgs/msg/Int32").unwrap();
        assert!(other.decode(&data).is_err());
        let mut wrong_version = data.clone();
        wrong_version[3] = 2;
        assert!(codec.decode(&wrong_version).is_err());
    }

    #[test]
    fn dynamic_service_request_response_cdr() {
        let (request, response) = service_codecs("example_interfaces/srv/AddTwoInts").unwrap();
        let mut message = request.message().unwrap();
        for (name, value) in [("a", -12), ("b", 54)] {
            if let Some(ValueMut::Simple(SimpleValueMut::Int64(x))) = message.get_mut(name) {
                *x = value;
            } else {
                panic!("missing int64 field");
            }
        }
        let bytes = request.encode(&message).unwrap();
        let result = request.decode(&bytes).unwrap();
        assert!(matches!(
            result.get("a"),
            Some(Value::Simple(SimpleValue::Int64(-12)))
        ));
        assert!(matches!(
            result.get("b"),
            Some(Value::Simple(SimpleValue::Int64(54)))
        ));
        assert!(response.decode(&bytes).is_err());
        response.message().unwrap();
    }
}
