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
    block_map: Vec<Vec<bool>>,
}
fn to_c_str(s: &IString) -> CString {
    let c_str = s.get_inner().to_owned();
    CString::new(c_str).unwrap()
}
pub fn draw_create(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(4, arg_num);
    let map_size = vm.gets_number() as usize;
    let t = vm.gets_string();
    let h = vm.gets_number();
    let w = vm.gets_number();
    let _ = vm.get_stack().pop();
    let mut b_draw_ctx = Box::new(DrawCtx {
        wnd_title: to_c_str(&t),
        camera: raylib::ffi::Camera3D {
            position: raylib::ffi::Vector3 {
                x: 0.,
                y: 10.,
                z: 10.,
            },
            target: raylib::ffi::Vector3 {
                x: 0.,
                y: 0.,
                z: 0.,
            },
            up: raylib::ffi::Vector3 {
                x: 0.,
                y: 1.,
                z: 0.,
            },
            fovy: 45.,
            projection: raylib::ffi::CameraProjection::CAMERA_PERSPECTIVE as i32,
        },
        block_map: vec![vec![false; map_size]; map_size],
    });
    unsafe {
        // memory management across ffi boundary is subtle
        raylib::ffi::InitWindow(w as i32, h as i32, b_draw_ctx.wnd_title.as_ptr());
        if !raylib::ffi::IsWindowReady() {
            panic!("raylib backend create window failed.");
        }
        raylib::ffi::SetTargetFPS(60);
    }
    let p_draw_ctx = Box::into_raw(b_draw_ctx);
    vm.get_stack()
        .push(Value::OpaqueData(p_draw_ctx as *mut u8));
}

pub fn draw_should_close(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let _ = vm.get_stack().pop();
    let _ = vm.get_stack().pop();
    let b = unsafe { raylib::ffi::WindowShouldClose() };
    vm.get_stack().push(Value::Bool(b));
}

pub fn draw_render_blocks(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let op_data = vm.gets_opaque() as *mut DrawCtx;
    let _ = vm.get_stack().pop();
    unsafe {
        let len = (*op_data).block_map.len();
        raylib::ffi::BeginDrawing();
        raylib::ffi::ClearBackground(raylib::color::Color::WHITE.into());
        raylib::ffi::BeginMode3D((*op_data).camera);
        for (i, v) in (*op_data).block_map.iter().enumerate() {
            for (j, b) in v.iter().enumerate() {
                if *b {
                    let position = raylib::ffi::Vector3 {
                        x: j as f32 + 0.5 - (len / 2) as f32,
                        y: 0.5,
                        z: i as f32 + 0.5 - (len / 2) as f32,
                    };
                    raylib::ffi::DrawCube(
                        position,
                        1.0,
                        1.0,
                        1.0,
                        raylib::color::Color::BLUEVIOLET.into(),
                    );
                }
            }
        }
        raylib::ffi::DrawGrid(len as i32, 1.0);
        raylib::ffi::EndMode3D();
        raylib::ffi::DrawFPS(10, 10);
        raylib::ffi::EndDrawing();
    }
    vm.get_stack().push(Value::Nil);
}

pub fn draw_destory(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let op_data = vm.gets_opaque() as *mut DrawCtx;
    let _ = vm.get_stack().pop();

    unsafe {
        let _to_drop = Box::from_raw(op_data);
        raylib::ffi::CloseWindow();
    }
    vm.get_stack().push(Value::Nil);
}

pub fn module_export() -> (String, Vec<(String, Value)>) {
    let module_name = "draw".to_owned();
    let module_func = vec![
        mf_entry!("create", draw_create),
        mf_entry!("should_close", draw_should_close),
        mf_entry!("render", draw_render_blocks),
        mf_entry!("destory", draw_destory),
    ];

    (module_name, module_func)
}
