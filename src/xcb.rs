use libc::*;
use std::ptr;
use std::mem;
use std::ffi::CStr;
use ffi::*;
use std::result::Result;
use std::borrow::ToOwned;
use std::fmt::Formatter;
use std::fmt::Debug;
use std::fmt::Error as FmtError;
use std::vec::Vec;
use std::ops::Deref;
use std::marker::PhantomData;
use std::str;
use std::slice;

#[derive(Debug)]
#[allow(raw_pointer_derive)]
pub struct XcbConnection {
    pub value: *mut xcb_connection_t,
    pub screen_num: c_int,
}

pub struct XcbSetup<'a> {
    value: &'a xcb_setup_t,
}

pub struct XcbRandr<'a> {
    connection: &'a XcbConnection,
    pub extension: xcb_query_extension_reply_t,
}

impl XcbConnection {
    pub fn new_default() -> XcbConnection {
        let mut screen_num = 0;
        let c = unsafe { xcb_connect(ptr::null(), &mut screen_num as *mut c_int) };
        XcbConnection { value: c, screen_num: screen_num }
    }
    
    pub fn get_setup<'a, 'b: 'a>(&'a self) -> XcbSetup<'a> {
      let s = unsafe { mem::transmute(xcb_get_setup(self.value)) };
      XcbSetup { value: s }
    }
    
    pub fn wait_for_event(&self) -> Result<LibcPtr<xcb_generic_event_t>, XcbError> {
        let event_ptr = unsafe { xcb_wait_for_event(self.value) };
        if event_ptr == 0 as *mut _ {
            Err(XcbError::IOError)
        } else {
            Ok(LibcPtr::new(event_ptr))
        }
    }
    
    pub fn intern_atom(&self, name: &str, only_if_exists: bool) -> Result<xcb_atom_t, XcbError> {
        let cookie = unsafe { xcb_intern_atom(self.value, only_if_exists as uint8_t, name.len() as uint16_t, name.as_ptr() as *const _) };
        let reply = try!(get_reply(self, cookie, xcb_intern_atom_reply));
        Ok(reply.atom)
    }
}

impl <'a> XcbSetup<'a> {
    pub fn roots_iterator(&'a self) -> XcbIterator<'a, xcb_screen_iterator_t, xcb_screen_t> {
        let ffi_it = unsafe { xcb_setup_roots_iterator(self.value) };
        XcbIterator::new(ffi_it, xcb_screen_next)
    }
}

impl Drop for XcbConnection {
    fn drop(&mut self) {
        unsafe { xcb_disconnect(self.value) };
    }
}

pub struct XcbIterator<'a, XcbIteratorType, ItemType> {
    marker: PhantomData<&'a XcbSetup<'a>>,
    marker2: PhantomData<ItemType>,
    iter: XcbIteratorType,
    iter_fn: unsafe extern "C" fn(*mut XcbIteratorType),
}

impl <'a, XcbIteratorType, ItemType> XcbIterator<'a, XcbIteratorType, ItemType> {
    pub fn new(iterator: XcbIteratorType, step_fn: unsafe extern "C" fn(*mut XcbIteratorType)) -> XcbIterator<'a, XcbIteratorType, ItemType> {
        XcbIterator {
            marker: PhantomData,
            marker2: PhantomData,
            iter: iterator,
            iter_fn: step_fn
        }
    }
}

impl <'a, XcbIteratorType, ItemType> Iterator for XcbIterator<'a, XcbIteratorType, ItemType> {
    type Item = &'a ItemType;
    fn next(&mut self) -> Option<&'a ItemType> {
        let cur;
        let rem;
        {
            let i = &self.iter as *const _ as *const xcb_generic_iterator_t;
            cur = unsafe { (*i).data };
            rem = unsafe { (*i).rem };
        }
        match rem {
            0 => None,
            _ => {
                let result = Some(unsafe { mem::transmute(cur) });
                unsafe { (self.iter_fn)(&mut self.iter as *mut _); }
                result
            }
        }
    }
}

pub struct LibcPtr<T> {
    pub value: *mut T
}

impl<T> LibcPtr<T> {
    pub fn new(ptr: *mut T) -> LibcPtr<T> {
        LibcPtr {
            value: ptr
        }
    }
}

// #[unsafe_destructor]
impl<T> Drop for LibcPtr<T> {
    fn drop(&mut self) {
        unsafe { free(self.value as *mut c_void) };
    }
}

impl<T> Deref for LibcPtr<T> {
    type Target = T;
    fn deref<'a>(&'a self) -> &'a T {
        unsafe { &*self.value }
    }
}

pub fn get_reply<TCookie, TResult>(
    connection: &XcbConnection,
    cookie: TCookie,
    reply_func: unsafe extern "C" fn (*mut xcb_connection_t, TCookie, *mut *mut xcb_generic_error_t) -> *mut TResult
) -> Result<LibcPtr<TResult>, xcb_generic_error_t> {
    let mut error_ptr = 0 as *mut _;
    let reply = unsafe { reply_func(connection.value, cookie, &mut error_ptr as *mut _) };
    
    if error_ptr != 0 as *mut _ {
        let result = Err(unsafe { *error_ptr });
        unsafe { free(error_ptr as *mut c_void) };
        result
    } else {
        let result = Ok(LibcPtr::new(reply));
        result
    }
}

pub fn wait_for_cookie(connection: &XcbConnection, cookie: xcb_void_cookie_t) -> Result<(), xcb_generic_error_t> {
    let error_ptr = unsafe { xcb_request_check(connection.value, cookie) };
    
    if error_ptr != 0 as *mut _ {
        let result = Err(unsafe { *error_ptr });
        unsafe { free(error_ptr as *mut c_void) };
        result
    } else {
        Ok(())
    }
}

impl Debug for xcb_generic_error_t {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), FmtError> {
        write!(
            fmt,
            "xcb_generic_error_t {{ error_code: {}, major_code: {}, minor_code: {} }}",
            self.error_code,
            self.major_code,
            self.minor_code
        )
    }
}


#[derive(Debug)]
pub enum XcbError {
    ProtoError(xcb_generic_error_t),
    LogicError(String),
    IOError,
}

impl From<xcb_generic_error_t> for XcbError {
    fn from(err: xcb_generic_error_t) -> XcbError {
        XcbError::ProtoError(err)
    }
}

impl <'a> XcbRandr<'a> {
    pub fn init(connection: &'a XcbConnection) -> Result<XcbRandr<'a>, XcbError> {
        let cookie = unsafe { xcb_query_extension(connection.value, 5, "RANDR".as_ptr() as *const c_char) };
        let reply = *try!(get_reply(connection, cookie, xcb_query_extension_reply));
        if reply.present == 0 {
            Err(XcbError::LogicError("RANDR extension is not present".to_owned()))
        } else {
            Ok(XcbRandr { connection: connection, extension: reply })
        }
    }
    
    pub fn get_screen_resources(&self, root_window_id: xcb_window_t) -> Result<XcbScreenResources, XcbError> {
        let cookie = unsafe { xcb_randr_get_screen_resources(self.connection.value, root_window_id) };
        let reply = try!(get_reply(self.connection, cookie, xcb_randr_get_screen_resources_reply));
        let crtcs = unsafe {
            slice::from_raw_parts(
                xcb_randr_get_screen_resources_crtcs(reply.value),
                xcb_randr_get_screen_resources_crtcs_length(reply.value) as usize
            )
        }.to_vec();
        let outputs = unsafe {
            slice::from_raw_parts(
                xcb_randr_get_screen_resources_outputs(reply.value),
                xcb_randr_get_screen_resources_outputs_length(reply.value) as usize
            )
        }.to_vec();
        let modes = unsafe {
            slice::from_raw_parts(
                xcb_randr_get_screen_resources_modes(reply.value),
                xcb_randr_get_screen_resources_modes_length(reply.value) as usize
            )
        }.to_vec();
        Ok(XcbScreenResources {
            config_timestamp: reply.config_timestamp,
            crtcs: crtcs,
            outputs: outputs,
            modes: modes,
            names: vec!(),
        })
    }

    pub fn get_output_info(&self, resources: &XcbScreenResources, output_id: xcb_randr_output_t) -> Result<XcbRandrOutputInfo, XcbError> {
        let cookie = unsafe { xcb_randr_get_output_info(self.connection.value, output_id, resources.config_timestamp) };
        let reply = try!(get_reply(self.connection, cookie, xcb_randr_get_output_info_reply));
        let name = String::from_utf8(
            unsafe {
                slice::from_raw_parts(
                    xcb_randr_get_output_info_name(reply.value) as *const u8,
                    xcb_randr_get_output_info_name_length(reply.value) as usize
                )
            }.to_vec()
        ).unwrap();
        Ok(XcbRandrOutputInfo {
            id: output_id,
            crtc: reply.crtc,
            mm_width: reply.mm_width,
            mm_height: reply.mm_height,
            connection: XcbRandrOutputConnectionStatus::from_u8(reply.connection).expect("Invalid connection status"),
            subpixel_order: reply.subpixel_order,
            name: name
        })
    }

    pub fn get_crtc_info(&self, resources: &XcbScreenResources, crtc_id: xcb_randr_crtc_t) -> Result<XcbRandrCrtcInfo, XcbError> {
        let cookie = unsafe { xcb_randr_get_crtc_info(self.connection.value, crtc_id, resources.config_timestamp) };
        let reply = try!(get_reply(self.connection, cookie, xcb_randr_get_crtc_info_reply));
        Ok(XcbRandrCrtcInfo {
            id: crtc_id,
            x: reply.x,
            y: reply.y,
            width: reply.width,
            height: reply.height,
        })
    }
    
    pub fn select_input(&self, window: xcb_window_t) -> Result<(), XcbError> {
        let cookie = unsafe {
            xcb_randr_select_input_checked(
                self.connection.value,
                window,
                XCB_RANDR_NOTIFY_MASK_SCREEN_CHANGE as u16 | 
                XCB_RANDR_NOTIFY_MASK_CRTC_CHANGE as u16 | 
                XCB_RANDR_NOTIFY_MASK_OUTPUT_CHANGE as u16 | 
                XCB_RANDR_NOTIFY_MASK_OUTPUT_PROPERTY as u16 | 
                XCB_RANDR_NOTIFY_MASK_PROVIDER_CHANGE as u16 | 
                XCB_RANDR_NOTIFY_MASK_PROVIDER_PROPERTY as u16 | 
                XCB_RANDR_NOTIFY_MASK_RESOURCE_CHANGE as u16
            )
        };
        try!(wait_for_cookie(self.connection, cookie));
        Ok(())
    }
}

#[derive(Debug)]
pub struct XcbXcreenResourceName;

#[derive(Debug)]
pub struct XcbScreenResources {
    pub config_timestamp: xcb_timestamp_t,
    pub crtcs: Vec<xcb_randr_crtc_t>,
    pub outputs: Vec<xcb_randr_output_t>,
    pub modes: Vec<xcb_randr_mode_info_t>,
    pub names: Vec<XcbXcreenResourceName>,
}

impl Debug for xcb_randr_mode_info_t {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), FmtError> {
        write!(
            fmt,
            "xcb_randr_mode_info_t {{ id: {}, width: {}, height: {}, doc_clock: {}, hsync_start: {}, hsync_end: {}, htotal: {}, hskew: {}, vsync_start: {}, vsync_end: {}, vtotal: {}, name_len: {}, mode_flags: {} }}",
            self.id,
            self.width,
            self.height,
            self.dot_clock,
            self.hsync_start,
            self.hsync_end,
            self.htotal,
            self.hskew,
            self.vsync_start,
            self.vsync_end,
            self.vtotal,
            self.name_len,
            self.mode_flags
        )
    }
}

#[derive(Debug)]
pub enum XcbRandrOutputConnectionStatus {
    Connected = 0,
    Disconnected = 1,
    Unknown = 2
}

impl XcbRandrOutputConnectionStatus {
    pub fn from_u8(val: u8) -> Option<XcbRandrOutputConnectionStatus> {
        match val {
            0 => Some(XcbRandrOutputConnectionStatus::Connected),
            1 => Some(XcbRandrOutputConnectionStatus::Disconnected),
            2 => Some(XcbRandrOutputConnectionStatus::Unknown),
            _ => None
        }
    }
}

#[derive(Debug)]
pub struct XcbRandrOutputInfo {
    pub id: xcb_randr_output_t,
    pub crtc: xcb_randr_crtc_t,
    pub mm_width: uint32_t,
    pub mm_height: uint32_t,
    pub connection: XcbRandrOutputConnectionStatus,
    pub subpixel_order: uint8_t,
    pub name: String
}

#[derive(Debug)]
pub struct XcbRandrCrtcInfo {
    pub id: xcb_randr_crtc_t,
    pub x: int16_t,
    pub y: int16_t,
    pub width: uint16_t,
    pub height: uint16_t,
}

#[derive(Debug)]
pub enum XcbRandrEventType {
    CrtcChange = 0,
    OutputChange = 1,
    OutputProperty = 2,
    ProviderChange = 3,
    ProviderProperty = 4,
    ResourceChange = 5,
}

impl XcbRandrEventType {
    pub fn from_u8(val: u8) -> Option<XcbRandrEventType> {
        match val {
            0 => Some(XcbRandrEventType::CrtcChange),
            1 => Some(XcbRandrEventType::OutputChange),
            2 => Some(XcbRandrEventType::OutputProperty),
            3 => Some(XcbRandrEventType::ProviderChange),
            4 => Some(XcbRandrEventType::ProviderProperty),
            5 => Some(XcbRandrEventType::ResourceChange),
            _ => None
        }
    }
}

pub struct XcbInput<'a> {
    pub connection: &'a XcbConnection,
    pub extension: xcb_query_extension_reply_t,
}

impl <'a> XcbInput<'a> {
    pub fn init(connection: &'a XcbConnection) -> Result<XcbInput<'a>, XcbError> {
        let xcb_input_extension_name = unsafe { CStr::from_ptr(xcb_input_id.name) };
        let cookie = unsafe { xcb_query_extension(connection.value, xcb_input_extension_name.to_bytes().len() as u16, xcb_input_extension_name.as_ptr()) };
        let reply = *try!(get_reply(connection, cookie, xcb_query_extension_reply));
        if reply.present == 0 {
            return Err(XcbError::LogicError(format!("{} extension is not present", str::from_utf8(xcb_input_extension_name.to_bytes()).unwrap())))
        }
        
        {
            let cookie = unsafe { xcb_input_xi_query_version(connection.value, 2, 3) };
            let reply = try!(get_reply(connection, cookie, xcb_input_xi_query_version_reply));
            
            if reply.major_version != 2 || reply.minor_version != 3 {
                return Err(XcbError::LogicError(format!("Invalid XINPUT version")));
            }
        }
        
        Ok(XcbInput { connection: connection, extension: reply })
    }
    
    pub fn get_devices(&self) -> Result<XcbInputDevices, XcbError> {
        let cookie = unsafe { xcb_input_xi_query_device(self.connection.value, 0) }; // 0 == AllDevices
        let reply = try!(get_reply(self.connection, cookie, xcb_input_xi_query_device_reply));
        
        let devices_it = XcbIterator::new(unsafe { xcb_input_xi_query_device_infos_iterator(reply.value) }, xcb_input_xi_device_info_next);
        let devices: Vec<_> = devices_it.map(|x| {
            let name = String::from_utf8(
                unsafe {
                    slice::from_raw_parts(
                        xcb_input_xi_device_info_name(x) as *const u8,
                        xcb_input_xi_device_info_name_length(x) as usize
                    )
                }.to_vec()
            ).unwrap();
            XcbInputDevice {
                deviceid: x.deviceid,
                _type: x._type,
                attachment: x.attachment,
                enabled: x.enabled != 0,
                name: name,
            }
        }).collect();
        
        Ok(XcbInputDevices {
            devices: devices,
        })
    }
    
    pub fn get_device_properties(&self, device_id: xcb_input_device_id_t) -> Result<Vec<String>, XcbError> {
        let cookie = unsafe { xcb_input_xi_list_properties(self.connection.value, device_id) };
        let reply = try!(get_reply(self.connection, cookie, xcb_input_xi_list_properties_reply));
        
        let atoms = unsafe {
            slice::from_raw_parts(
                xcb_input_xi_list_properties_properties(reply.value),
                xcb_input_xi_list_properties_properties_length(reply.value) as usize
            )
        }.to_vec();
        
        let names_wrapped: Vec<_> = atoms
            .iter()
            .map(|atom| unsafe { xcb_get_atom_name(self.connection.value, *atom) })
            .map(|atom_cookie| get_reply(self.connection, atom_cookie, xcb_get_atom_name_reply))
            .collect();
            
        {
            let first_error = { names_wrapped.iter().filter(|x| x.is_err()).next() };
            
            match first_error {
                Some(&Err(ref e)) => return Err(From::from(*e)),
                _ => {}
            }
        }
        
        let names: Vec<_> = names_wrapped.into_iter().map(|x| {
            let reply = x.unwrap();
            String::from_utf8(
                unsafe {
                    slice::from_raw_parts(
                        xcb_get_atom_name_name(reply.value) as *const u8,
                        xcb_get_atom_name_name_length(reply.value) as usize
                    )
                }.to_vec()
            ).unwrap()
        }).collect();
        
        Ok(names)
    }
    
    pub fn set_property_value<PropT>(
        &self, device_id: xcb_input_device_id_t,
        property: xcb_atom_t, proptype: xcb_atom_t,
        format: u8,
        data: &[PropT]
    ) -> Result<(), XcbError>
    {
        let cookie = unsafe {
            xcb_input_xi_change_property(
                self.connection.value, //xcb_connection_t *c
                device_id, //xcb_input_device_id_t deviceid
                XCB_PROP_MODE_REPLACE as uint8_t, // uint8_t mode
                format, //uint8_t format
                property, //xcb_atom_t property
                proptype, //xcb_atom_t type
                data.len() as uint32_t, //uint32_t num_items
                data.as_ptr() as *const c_void //const void *items
            )
        };
        try!(wait_for_cookie(self.connection, cookie));
        Ok(())
    }
    
    pub fn select_device_add_remove_events(&self, root_window_id: xcb_window_t) -> Result<(), XcbError> {
        let mask = XcbInputEventMask {
            xcb_data: xcb_input_event_mask_t {
                deviceid: 0, // AllDevices == 0
                mask_len: 1,
            },
            mask_val: XCB_INPUT_XI_EVENT_MASK_HIERARCHY
        };
        let cookie = unsafe {
            xcb_input_xi_select_events(self.connection.value, root_window_id, 1, &mask as *const _ as *const xcb_input_event_mask_t)
        };
        try!(wait_for_cookie(self.connection, cookie));
        Ok(())
    }
}

#[repr(C)]
struct XcbInputEventMask {
    pub xcb_data: xcb_input_event_mask_t,
    pub mask_val: uint32_t,
}

#[derive(Debug)]
pub struct XcbInputDevices {
    pub devices: Vec<XcbInputDevice>,
}

#[derive(Debug)]
pub struct XcbInputDevice {
    pub deviceid: xcb_input_device_id_t,
    pub _type: uint16_t,
    pub attachment: xcb_input_device_id_t,
    pub enabled: bool,
    pub name: String,
}