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

macro_rules! num_entry {
    ($name:expr, $num:expr) => {
        ($name.to_owned(), Value::Number($num as f64))
    };
}

macro_rules! export_enum {
    ($($k:ident = $v:expr),*) => {
        vec![$((stringify!($k).to_owned(), Value::Number($v as f64))),*]
    };
}
struct DrawCtx {
    wnd_title: CString,
    camera: raylib::ffi::Camera3D,
    block_map: Vec<Vec<u32>>,
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
        block_map: vec![vec![0; map_size]; map_size],
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

pub fn draw_set_block(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(4, arg_num);
    let color = vm.gets_number();
    let j = vm.gets_number();
    let i = vm.gets_number();
    let op_data = vm.gets_opaque() as *mut DrawCtx;
    let _ = vm.get_stack().pop();
    unsafe {
        (*op_data).block_map[i as usize][j as usize] = color as u32;
    }

    vm.get_stack().push(Value::Nil);
}

pub fn draw_get_block(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(3, arg_num);
    let j = vm.gets_number();
    let i = vm.gets_number();
    let op_data = vm.gets_opaque() as *mut DrawCtx;
    let _ = vm.get_stack().pop();
    unsafe {
        let color = (*op_data).block_map[i as usize][j as usize];
        vm.get_stack().push(Value::Number(color as f64));
    }
}

pub fn draw_set_camera_pos(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(4, arg_num);
    let z = vm.gets_number();
    let y = vm.gets_number();
    let x = vm.gets_number();
    let op_data = vm.gets_opaque() as *mut DrawCtx;
    let _ = vm.get_stack().pop();

    unsafe {
        (*op_data).camera.position = raylib::ffi::Vector3 {
            x: x as f32,
            y: y as f32,
            z: z as f32,
        };
    }

    vm.get_stack().push(Value::Nil);
}

pub fn draw_is_key_pressed(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(2, arg_num);
    let key = vm.gets_number();
    let op_data = vm.gets_opaque() as *mut DrawCtx;
    let _ = vm.get_stack().pop();
    unsafe {
        let pressed = raylib::ffi::IsKeyDown(key as i32);
        vm.get_stack().push(Value::Bool(pressed));
    }
}

pub fn draw_get_frame_time(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let op_data = vm.gets_opaque() as *mut DrawCtx;
    let _ = vm.get_stack().pop();
    unsafe {
        let frame_time = raylib::ffi::GetFrameTime();
        vm.get_stack().push(Value::Number(frame_time as f64));
    }
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
                if b > &0 {
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
                        raylib::color::Color::get_color(*b).into(),
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

    let mut module_func = vec![
        mf_entry!("create", draw_create),
        mf_entry!("should_close", draw_should_close),
        mf_entry!("render", draw_render_blocks),
        mf_entry!("destory", draw_destory),
        mf_entry!("set_block", draw_set_block),
        mf_entry!("get_block", draw_get_block),
        mf_entry!("set_camera", draw_set_camera_pos),
        mf_entry!("is_key_pressed", draw_is_key_pressed),
        mf_entry!("get_frame_time", draw_get_frame_time),
    ];
    let mut keys = export_enum!(
        KEY_NULL = 0,
        KEY_APOSTROPHE = 39,
        KEY_COMMA = 44,
        KEY_MINUS = 45,
        KEY_PERIOD = 46,
        KEY_SLASH = 47,
        KEY_ZERO = 48,
        KEY_ONE = 49,
        KEY_TWO = 50,
        KEY_THREE = 51,
        KEY_FOUR = 52,
        KEY_FIVE = 53,
        KEY_SIX = 54,
        KEY_SEVEN = 55,
        KEY_EIGHT = 56,
        KEY_NINE = 57,
        KEY_SEMICOLON = 59,
        KEY_EQUAL = 61,
        KEY_A = 65,
        KEY_B = 66,
        KEY_C = 67,
        KEY_D = 68,
        KEY_E = 69,
        KEY_F = 70,
        KEY_G = 71,
        KEY_H = 72,
        KEY_I = 73,
        KEY_J = 74,
        KEY_K = 75,
        KEY_L = 76,
        KEY_M = 77,
        KEY_N = 78,
        KEY_O = 79,
        KEY_P = 80,
        KEY_Q = 81,
        KEY_R = 82,
        KEY_S = 83,
        KEY_T = 84,
        KEY_U = 85,
        KEY_V = 86,
        KEY_W = 87,
        KEY_X = 88,
        KEY_Y = 89,
        KEY_Z = 90,
        KEY_LEFT_BRACKET = 91,
        KEY_BACKSLASH = 92,
        KEY_RIGHT_BRACKET = 93,
        KEY_GRAVE = 96,
        KEY_SPACE = 32,
        KEY_ESCAPE = 256,
        KEY_ENTER = 257,
        KEY_TAB = 258,
        KEY_BACKSPACE = 259,
        KEY_INSERT = 260,
        KEY_DELETE = 261,
        KEY_RIGHT = 262,
        KEY_LEFT = 263,
        KEY_DOWN = 264,
        KEY_UP = 265,
        KEY_PAGE_UP = 266,
        KEY_PAGE_DOWN = 267,
        KEY_HOME = 268,
        KEY_END = 269,
        KEY_CAPS_LOCK = 280,
        KEY_SCROLL_LOCK = 281,
        KEY_NUM_LOCK = 282,
        KEY_PRINT_SCREEN = 283,
        KEY_PAUSE = 284,
        KEY_F1 = 290,
        KEY_F2 = 291,
        KEY_F3 = 292,
        KEY_F4 = 293,
        KEY_F5 = 294,
        KEY_F6 = 295,
        KEY_F7 = 296,
        KEY_F8 = 297,
        KEY_F9 = 298,
        KEY_F10 = 299,
        KEY_F11 = 300,
        KEY_F12 = 301,
        KEY_LEFT_SHIFT = 340,
        KEY_LEFT_CONTROL = 341,
        KEY_LEFT_ALT = 342,
        KEY_LEFT_SUPER = 343,
        KEY_RIGHT_SHIFT = 344,
        KEY_RIGHT_CONTROL = 345,
        KEY_RIGHT_ALT = 346,
        KEY_RIGHT_SUPER = 347,
        KEY_KB_MENU = 348,
        KEY_KP_0 = 320,
        KEY_KP_1 = 321,
        KEY_KP_2 = 322,
        KEY_KP_3 = 323,
        KEY_KP_4 = 324,
        KEY_KP_5 = 325,
        KEY_KP_6 = 326,
        KEY_KP_7 = 327,
        KEY_KP_8 = 328,
        KEY_KP_9 = 329,
        KEY_KP_DECIMAL = 330,
        KEY_KP_DIVIDE = 331,
        KEY_KP_MULTIPLY = 332,
        KEY_KP_SUBTRACT = 333,
        KEY_KP_ADD = 334,
        KEY_KP_ENTER = 335,
        KEY_KP_EQUAL = 336,
        KEY_BACK = 4,
        KEY_VOLUME_UP = 24,
        KEY_VOLUME_DOWN = 25
    );
    module_func.append(&mut keys);
    (module_name, module_func)
}
