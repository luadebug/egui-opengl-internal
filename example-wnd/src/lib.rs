use std::{intrinsics::transmute, sync::Once};
use std::ffi::c_void;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

use egui::{Color32, Context, Key, Modifiers, RichText, ScrollArea, Slider, Widget};
use once_cell::unsync::Lazy;
use retour::static_detour;
use windows::{
    core::HRESULT,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{HDC, WindowFromDC},
        UI::WindowsAndMessaging::{CallWindowProcW, GWLP_WNDPROC, WNDPROC},
    },
};
use windows::Win32::Foundation::{BOOL, TRUE};
use windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;

use egui_opengl_internal::{OpenGLApp, utils};

struct UIState {
    ui_check: bool,
    text: String,
    value: f32,
    color: [f32; 3],
}

impl UIState {
    fn new() -> Self {
        Self {
            ui_check: true,
            text: String::from("Test"),
            value: 0.0,
            color: [0.0, 0.0, 0.0],
        }
    }
}
static mut STATE: Lazy<Arc<Mutex<UIState>>> = Lazy::new(|| Arc::new(Mutex::new(UIState::new())));



#[no_mangle]
extern "system" fn DllMain(hinst: usize, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == 1 {
        std::thread::spawn(move || unsafe { main_thread(hinst) });
    }

    if reason == 0 {
        unsafe {
            WglSwapBuffersHook.disable().unwrap();
            let wnd_proc = OLD_WND_PROC.unwrap().unwrap();
            let _: Option<WNDPROC> = Some(transmute::<i32,
                                    Option<unsafe extern "system"
                                    fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>>(
                                    SetWindowLongPtrW(
                APP.get_window(),
                GWLP_WNDPROC,
                wnd_proc as usize as _,
            )));

            utils::free_console();
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    TRUE
}

static mut APP: OpenGLApp<i32> = OpenGLApp::new();
static mut OLD_WND_PROC: Option<WNDPROC> = None;
static mut EXITING: bool = false;

type FnWglSwapBuffers = unsafe extern "stdcall" fn(HDC) -> HRESULT;
static_detour! {
    static WglSwapBuffersHook: unsafe extern "stdcall" fn(HDC) -> HRESULT;
}

fn hk_wgl_swap_buffers(hdc: HDC) -> HRESULT {
    unsafe {
        let window = WindowFromDC(hdc);

        static INIT: Once = Once::new();
        INIT.call_once(|| {
            println!("wglSwapBuffers successfully hooked.");

            APP.init_default(hdc, window, ui);

            OLD_WND_PROC = Some(transmute::<i32, Option<unsafe extern "system"
                    fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>>(
                    SetWindowLongPtrW(
                window,
                GWLP_WNDPROC,
                hk_wnd_proc as usize as _,
            )));
        });

        if !APP.get_window().eq(&window) {
            SetWindowLongPtrW(window, GWLP_WNDPROC, hk_wnd_proc as usize as _);
        }

        APP.render(hdc);
        WglSwapBuffersHook.call(hdc)
    }
}

unsafe extern "stdcall" fn hk_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        println!("CallWindowProcW successfully hooked.");
    });

    let egui_wants_input = APP.wnd_proc(msg, wparam, lparam);
    if egui_wants_input {
        return LRESULT(1);
    }

    CallWindowProcW(OLD_WND_PROC.unwrap(), hwnd, msg, wparam, lparam)
}

fn ui(ctx: &Context, _: &mut i32) {
    unsafe {
        egui::containers::Window::new("Main menu").show(ctx, |ui| {
            test_ui(ctx, ui);

            ui.separator();
            if ui.button("exit").clicked() {
                EXITING = true;
            }
        });
    }
}

unsafe fn main_thread(_hinst: usize) {
    utils::alloc_console();

    let wgl_swap_buffers = utils::get_proc_address("wglSwapBuffers");
    let fn_wgl_swap_buffers: FnWglSwapBuffers = transmute(wgl_swap_buffers);

    println!("wglSwapBuffers: {:X}", wgl_swap_buffers as usize);

    WglSwapBuffersHook
        .initialize(fn_wgl_swap_buffers, hk_wgl_swap_buffers)
        .unwrap()
        .enable()
        .unwrap();

    #[allow(clippy::empty_loop)]
    while !EXITING {}
    utils::unload();
}

unsafe fn test_ui(ctx: &Context, ui: &mut egui::Ui) {
    let state = STATE.as_ref();

    // UI Elements
    ui.label(RichText::new("Test").color(Color32::LIGHT_BLUE));
    ui.label(RichText::new("Other").color(Color32::WHITE));
    ui.separator();

    let input = ctx.input(|input| input.pointer.clone());
    ui.label(format!(
        "X1: {} X2: {}",
        input.button_down(egui::PointerButton::Extra1),
        input.button_down(egui::PointerButton::Extra2)
    ));

    let mods = ui.input(|input| input.modifiers);
    ui.label(format!(
        "Ctrl: {} Shift: {} Alt: {}",
        mods.ctrl, mods.shift, mods.alt
    ));

    if ui.input(|input| input.modifiers.matches_exact(Modifiers::CTRL) && input.key_pressed(Key::R)) {
        println!("Pressed");
    }

    // Checkbox and Text Input
    let mut binding = state.lock().unwrap();
    let ui_state = binding.deref_mut();
    if ui.checkbox(&mut ui_state.ui_check, "Some checkbox").changed() {
        println!("Checkbox toggled to: {}", ui_state.ui_check);
    }
    if ui.text_edit_singleline(&mut ui_state.text).changed()
    {
        println!("Set edit singleline to: {}", ui_state.text);
    }

    // Scroll Area
    ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
        for i in 1..=100 {
            ui.label(format!("Label: {}", i));
        }
    });

    // Slider
    if Slider::new(&mut ui_state.value, -1.0..=1.0).ui(ui).changed()
    {
        println!("Slider set value to: {}", ui_state.value);
    }

    // Color Picker
    if ui.color_edit_button_rgb(&mut ui_state.color).changed()
    {
        println!("Color edit button set color to: {:?}", ui_state.color);
    }

    // Display Pointer Info
    ui.label(format!(
        "{:?}",
        &ui.input(|input| input.pointer.button_down(egui::PointerButton::Primary))
    ));
}