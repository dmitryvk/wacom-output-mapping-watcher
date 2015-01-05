#![feature(globs)]
#![feature(unsafe_destructor)]
#![feature(associated_types)]

extern crate getopts;
extern crate libc;

use xcb::*;
use std::num::FromPrimitive;
use std::io::timer::sleep;
use std::time::duration::Duration;
use getopts::{optflag,optopt,getopts,usage};
use std::os;

// FFI is build with:
// LD_PRELOAD=/usr/lib/libclang.so ./bindgen -lxcb -lxcb-randr -lxcb-xinput -I /usr/lib/clang/3.5.0/include -match /usr/include/xcb/ -o ~/develop/rust-wacom-randr/src/ffi.rs ~/develop/rust-wacom-randr/src/ffi-input.h
mod ffi {
    #![allow(dead_code, non_camel_case_types, raw_pointer_deriving, non_snake_case)]
    use libc::*;
    
    #[repr(C)]
    pub struct Struct_iovec {
        pub iov_base: *mut c_void,
        pub iov_len: size_t,
    }
    
    include!("ffi.rs");
}
mod xcb;

struct CliOptions {
    pub watch: bool,
    pub output: String,
}

fn parse_options() -> Option<CliOptions> {
    let args: Vec<String> = os::args();

    let program = args[0].clone();

    let opts = &[
        optflag("w", "watch", "watch for RANDR events and reconfigure Wacom tablets"),
        optopt("o", "output", "name of X RANDR output to which Wacom tables will be mapped", "OUTPUT"),
        optflag("h", "help", "print this help menu")
    ];
    let matches = match getopts(args.tail(), opts) {
        Ok(m) => { m }
        Err(f) => { panic!(f.to_string()) }
    };
    if matches.opt_present("h") || !matches.opt_present("o") {
        let brief = format!("Usage: {} [options]", program);
        print!("{}", usage(brief.as_slice(), opts));
        return None;
    }
    
    return Some(CliOptions {
        watch: matches.opt_present("w"),
        output: matches.opt_str("o").unwrap()
    });
}

fn get_active_outputs(randr: &XcbRandr, resources: &XcbScreenResources) -> Vec<(XcbRandrOutputInfo, XcbRandrCrtcInfo)> {
    let result = resources
        .outputs
        .iter()
        .map(|output_id| randr.get_output_info(resources, *output_id).unwrap())
        .filter(|output_info| if let XcbRandrOutputConnectionStatus::Connected = output_info.connection { true } else { false })
        .filter(|output_info| output_info.crtc != 0)
        .map(|output_info| {
            let crtc_info = randr.get_crtc_info(resources, output_info.crtc).unwrap();
            (output_info, crtc_info)
        })
        .collect();
    result
}

#[derive(Show,PartialEq,Eq)]
pub struct XcbOutputDescription {
    pub name: String,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

fn describe_output_and_crtc(x: &(XcbRandrOutputInfo, XcbRandrCrtcInfo)) -> XcbOutputDescription {
    XcbOutputDescription {
        name: x.0.name.clone(),
        x: x.1.x,
        y: x.1.y,
        width: x.1.width,
        height: x.1.height,
    }
}

fn update_wacom_tablets(connection: &XcbConnection, input: &XcbInput, outputs: &[XcbOutputDescription], to_out_name: &str) {
    let to_out_opt = outputs.iter().filter(|o| o.name.as_slice() == to_out_name).nth(0);
    let to_out = match to_out_opt {
        Some(out) => out,
        None => outputs.get(0).unwrap()
    };
    let min_x: f32 = outputs.iter().map(|o| o.x).min().unwrap() as f32;
    let min_y: f32 = outputs.iter().map(|o| o.y).min().unwrap() as f32;
    let max_x: f32 = outputs.iter().map(|o| o.x + o.width as i16).max().unwrap() as f32;
    let max_y: f32 = outputs.iter().map(|o| o.y + o.height as i16).max().unwrap() as f32;
    
    let dx = (to_out.x as f32 - min_x) / (max_x - min_x);
    let dy = (to_out.y as f32 - min_y) / (max_y - min_y);
    let cx = to_out.width as f32 / (max_x - min_x);
    let cy = to_out.height as f32 / (max_y - min_y);
    
    let transform_matrix = vec!(
         cx,  0.0,   dx,
        0.0,   cy,   dy,
        0.0,  0.0,  1.0
    );
    
    for device in input.get_devices().unwrap().devices.iter() {
        if device.name.starts_with("Wacom") {
            for property in input.get_device_properties(device.deviceid).unwrap().iter() {
                if property.as_slice() == "Coordinate Transformation Matrix" {
                    let property_name_atom = connection.intern_atom(property.as_slice(), true).unwrap();
                    let property_type_atom = connection.intern_atom("FLOAT", true).unwrap();
                    println!("Updating {}", device.name);
                    input.set_property_value(device.deviceid, property_name_atom, property_type_atom, 32, transform_matrix.as_slice()).unwrap();
                    
                    /*{
                        let data = input.get_property_value::<f32>(device.deviceid, property_name_atom);
                        println!("    Value data: {}", data);
                    }*/
                }
            }
        }
    }
}

fn main() {
    let options = parse_options();
    
    if let None = options {
        return;
    }
    
    let options = options.unwrap();

    let c = XcbConnection::new_default();
    let setup = c.get_setup();
    let root_window_id = setup.roots_iterator().nth(0).unwrap().root;
    let randr = XcbRandr::init(&c).unwrap();
    
    let resources = randr.get_screen_resources(root_window_id).unwrap();
    
    let active_outputs: Vec<_> = get_active_outputs(&randr, &resources)
        .iter()
        .map(describe_output_and_crtc)
        .collect();
    println!("Active outputs: {}", active_outputs);
    
    let input = XcbInput::init(&c).unwrap();
    
    update_wacom_tablets(&c, &input, active_outputs.as_slice(), options.output.as_slice());
    
    if options.watch {
        randr.select_input(root_window_id).unwrap();
        input.select_device_add_remove_events(root_window_id).unwrap();
        
        let mut prev_outputs = active_outputs;
        
        loop {
            let event = c.wait_for_event().unwrap();
            
            if event.response_type >= randr.extension.first_event
                && event.response_type <= randr.extension.first_event + (ffi::XCB_RANDR_NOTIFY_RESOURCE_CHANGE as u8)
            {
                let event_type: XcbRandrEventType = FromPrimitive::from_u8(event.response_type - randr.extension.first_event).expect("Invalid value");
                let active_outputs: Vec<_> = get_active_outputs(&randr, &resources)
                    .iter()
                    .map(describe_output_and_crtc)
                    .collect();
                if active_outputs != prev_outputs {
                    println!("Active outputs have changed from {} to {}", prev_outputs, active_outputs);
    
                    update_wacom_tablets(&c, &input, active_outputs.as_slice(), options.output.as_slice());
                    prev_outputs = active_outputs;
                }
            } else if event.response_type == 35 /* XCB_GE_GENERIC */ {
                let ge = unsafe { &*(event.value as *const ffi::xcb_ge_generic_event_t) };
                if ge.extension == input.extension.major_opcode {
                    if ge.event_type == 11 /* XINPUT Hierarchy event */ {
                        println!("Device hierarchy changed");
                        //sleep(Duration::seconds(1));
                        update_wacom_tablets(&c, &input, prev_outputs.as_slice(), options.output.as_slice());
                    }
                }
            }
        }
    }
}