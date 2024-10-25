//! simple graphic interface for sloth-lang
//! using raylib
//! the main purpose is to support `game of life` & `snake`
use std::ffi::{CStr, CString};

use raylib::prelude::*;

use crate::{interned_string::IString, vm::Vm, Value};
macro_rules! arity_assert {
    ($n:expr, $arg_num:expr) => {
        if $arg_num != $n {
            panic!("arity check failed, passed {}, required {}", $n, $arg_num);
        }
    };
}

macro_rules! mf_entry {
    ($name:expr,$func:expr) => {
        ($name.to_owned(), Value::NativeFunction($func as *mut u8))
    };
}


struct DrawCtx {
    wnd_title: CString,
    camera: raylib::ffi::Camera3D,
}
fn to_c_str(s: &IString) -> CString {
    let c_str = s.get_inner().to_owned();
    CString::new(c_str).unwrap()
}
pub fn draw_world_init(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(3, arg_num);
    let h = vm.gets_number();
    let w = vm.gets_number();
    let t = vm.gets_string();
    let _ = vm.get_stack().pop();
    let mut b_draw_ctx = Box::new(DrawCtx{
        wnd_title: to_c_str(&t),
        camera: raylib::ffi::Camera3D{
            position: raylib::ffi::Vector3{x: 0., y: 10., z:10.},
            target: raylib::ffi::Vector3{x:0., y: 0., z: 0.},
            up :raylib::ffi::Vector3{x:0., y: 1., z: 0.},
            fovy: 45.,
            projection: raylib::ffi::CameraProjection::CAMERA_PERSPECTIVE as i32,
        },
    });
    let p_draw_ctx = b_draw_ctx.as_mut() as *mut DrawCtx;
    unsafe {
        // memory management across ffi boundary is subtle
        raylib::ffi::InitWindow(w as i32, h as i32, b_draw_ctx.wnd_title.as_ptr());
        if !raylib::ffi::IsWindowReady() {
            panic!("raylib backend create window failed.");
        }
        raylib::ffi::SetTargetFPS(60);

    }
    vm.get_stack().push(Value::OpaqueData(p_draw_ctx as *mut u8));
}


pub fn draw_should_close(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(0, arg_num);
    let _ = vm.get_stack().pop();
    let b = unsafe {
        raylib::ffi::WindowShouldClose()
    };
    vm.get_stack().push(Value::Bool(b));
}





