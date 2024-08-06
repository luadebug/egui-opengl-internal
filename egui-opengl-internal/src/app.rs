use std::ffi::c_void;
use crate::{input::InputCollector, painter, utils};
use clipboard::{windows_clipboard::WindowsClipboardContext, ClipboardProvider};
use egui::{Context, FontData, FontDefinitions, FontId};
use once_cell::sync::OnceCell;
use std::ops::DerefMut;
use std::path::PathBuf;
use egui::FontFamily::Proportional;
use egui::TextStyle::{Body, Button, Heading, Monospace, Name, Small};
use windows::Win32::{
    Foundation::{HWND, LPARAM, RECT, WPARAM},
    Graphics::{
        Gdi::{WindowFromDC, HDC},
        OpenGL::{wglCreateContext, wglGetCurrentContext, wglMakeCurrent, HGLRC},
    },
    UI::WindowsAndMessaging::{GetClientRect, WM_SIZE},
};

#[allow(clippy::type_complexity)]
struct AppData<T> {
    ui: Box<dyn FnMut(&Context, &mut T) + 'static>,
    gl_context: HGLRC,
    window: HWND,
    painter: painter::Painter,
    input_collector: InputCollector,
    ctx: Context,
    client_rect: (u32, u32),
    state: T,
}

#[cfg(feature = "parking-lot")]
use parking_lot::{Mutex, MutexGuard};
#[cfg(feature = "spin-lock")]
use spin::lock_api::{Mutex, MutexGuard};

use lock_api::MappedMutexGuard;


/// Heart and soul of this integration.
/// Main methods you are going to use are:
/// * [`Self::render`] - Should be called inside of wglSwapBuffers hook.
/// * [`Self::wnd_proc`] - Should be called on each `WndProc`.
pub struct OpenGLApp<T = ()> {
    data: Mutex<Option<AppData<T>>>,
    hwnd: OnceCell<HWND>,
}

impl<T> Default for OpenGLApp<T> {
    fn default() -> Self {
        Self::new()
    }
}



impl<T> OpenGLApp<T> {
    /// Creates new [`OpenGLApp`] in const context. You are supposed to create a single static item to store the application state.
    pub const fn new() -> Self {
        Self {
            data: Mutex::new(None),
            hwnd: OnceCell::new(),
        }
    }

    /// Checks if the app is ready to draw and if it's safe to invoke `render`, `wndproc`, etc.
    /// `true` means that you have already called an `init_*` on the application.
    pub fn is_ready(&self) -> bool {
        self.hwnd.get().is_some()
    }

    /// Initializes application and state. You should call this only once!
    pub fn init_with_state_context(
        &self,
        hdc: HDC,
        window: HWND,
        ui: impl FnMut(&Context, &mut T) + 'static,
        state: T,
        context: Context,
    ) {
        unsafe {
            if self.hwnd.get().is_some() {
                panic_msg!("You must call init only once");
            }

            if window.0 == (-1i32 as *mut c_void) {
                panic_msg!("Invalid output window descriptor");
            }

            let _ = self.hwnd.set(window);

            // loads gl with all the opengl functions using get_proc_address which is hardcoded to look in the opengl32.dll module
            gl::load_with(|s| utils::get_proc_address(s) as *const _);

            let o_context = wglGetCurrentContext();
            let gl_context = wglCreateContext(hdc).unwrap();
            wglMakeCurrent(hdc, gl_context).unwrap();

            let painter = painter::Painter::new();

            *self.data.lock() = Some(AppData {
                input_collector: InputCollector::new(window),
                ui: Box::new(ui),
                gl_context,
                window,
                ctx: context,
                client_rect: (0, 0),
                state,
                painter,
            });

            wglMakeCurrent(hdc, o_context).unwrap();
        }
    }

    /// Initializes application and state. Sets egui's context to default value. You should call this only once!
    #[inline]
    pub fn init_with_state(
        &self,
        hdc: HDC,
        window: HWND,
        ui: impl FnMut(&Context, &mut T) + 'static,
        state: T,
    ) {
        self.init_with_state_context(hdc, window, ui, state, Context::default())
    }

    /// Initializes application and state while allowing you to mutate the initial state of the egui's context. You should call this only once!
    #[inline]
    pub fn init_with_mutate(
        &self,
        hdc: HDC,
        window: HWND,
        ui: impl FnMut(&Context, &mut T) + 'static,
        mut state: T,
        mutate: impl FnOnce(&mut Context, &mut T),
    ) {
        let mut ctx = Context::default();
        mutate(&mut ctx, &mut state);

        self.init_with_state_context(hdc, window, ui, state, ctx);
    }

    #[cfg(feature = "parking-lot")]
    pub fn lock_state(&self) -> MappedMutexGuard<'_, parking_lot::RawMutex, T> {
        MutexGuard::map(self.data.lock(), |app| &mut app.as_mut().unwrap().state)
    }

    #[cfg(feature = "spin-lock")]
    pub fn lock_state(&self) -> MappedMutexGuard<'_, spin::mutex::Mutex<()>, T> {
        MutexGuard::map(self.data.lock(), |app| &mut app.as_mut().unwrap().state)
    }

    fn lock_data(&self) -> impl DerefMut<Target = AppData<T>> + '_ {
        MutexGuard::map(self.data.lock(), |app| {
            expect!(app.as_mut(), "You need to call init first")
        })
    }
}

impl<T: Default> OpenGLApp<T> {
    /// Initializes application and sets the state to its default value. You should call this only once!
    #[inline]
    pub fn init_default(&self, hdc: HDC, window: HWND, ui: impl FnMut(&Context, &mut T) + 'static) {
        let ctx = Context::default();

        let font_file = {
            let mut font_path = PathBuf::from(std::env::var("SystemRoot").ok().unwrap());
            font_path.push("Fonts");
            font_path.push("arial.ttf");
            font_path.to_str().unwrap().to_string().replace("\\", "/")
        };
        let font_name = font_file.split('/').last().unwrap().split('.').next().unwrap().to_string();
        let font_file_bytes = std::fs::read(font_file).ok().unwrap();

        let font_data = FontData::from_owned(font_file_bytes);
        let mut font_def = FontDefinitions::default();
        font_def.font_data.insert(font_name.to_string(), font_data);

        let font_family = Proportional;
        font_def.families.get_mut(&font_family).unwrap().insert(0, font_name);

        ctx.set_fonts(font_def);


        // Set custom sizes for text styles
        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (Heading, FontId::new(30.0, Proportional)),
            (Name("Heading2".into()), FontId::new(25.0, Proportional)),
            (Name("Context".into()), FontId::new(23.0, Proportional)),
            (Body, FontId::new(18.0, Proportional)),
            (Monospace, FontId::new(14.0, Proportional)),
            (Button, FontId::new(14.0, Proportional)),
            (Small, FontId::new(10.0, Proportional)),
        ].into();

        ctx.set_style(style);
        self.init_with_state_context(hdc, window, ui, T::default(), ctx);
    }
}

impl<T> OpenGLApp<T> {
    /// Present call. Should be called once per original present call, before or inside of hook.
    #[allow(invalid_reference_casting)]
    pub fn render(&self, hdc: HDC) {
        unsafe {
            let this = &mut *self.lock_data();

            let window = WindowFromDC(hdc);
            if !window.eq(&this.window) {
                this.window = window;
                this.input_collector = InputCollector::new(window);
                this.client_rect = self.get_client_rect(this.window);
            }

            let o_context = wglGetCurrentContext();
            wglMakeCurrent(hdc, this.gl_context).unwrap();

            let output = this
                .ctx
                .run(this.input_collector.collect_input(&this.ctx), |ctx| {
                    (this.ui)(ctx, &mut this.state);
                });

            if !output.platform_output.copied_text.is_empty() {
                let _ = WindowsClipboardContext.set_contents(output.platform_output.copied_text);
            }

            if output.shapes.is_empty() {
                wglMakeCurrent(hdc, o_context).unwrap();
                return;
            }

            let client_rect = self.poll_client_rect(this);
            let clipped_shapes = this.ctx.tessellate(output.shapes, 1.);
            this.painter.paint_and_update_textures(
                1.0,
                &clipped_shapes,
                &output.textures_delta,
                &client_rect,
            );

            wglMakeCurrent(hdc, o_context).unwrap();
        }
    }

    /// Call on each `WndProc` occurence.
    /// Returns `true` if message was recognized and dispatched by input handler,
    /// `false` otherwise.
    #[inline]
    pub fn wnd_proc(&self, umsg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
        let this = &mut *self.lock_data();



        this.input_collector.process(umsg, wparam.0, lparam.0);

        if umsg == WM_SIZE {
            this.client_rect = self.get_client_rect(this.window);
        }



        this.ctx.wants_keyboard_input() || this.ctx.wants_pointer_input()
    }

    pub fn get_window(&self) -> HWND {
        let data = &mut *self.lock_data();
        data.window
    }
}

impl<T> OpenGLApp<T> {
    #[inline]
    fn poll_client_rect(&self, data: &mut AppData<T>) -> (u32, u32) {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            data.client_rect = self.get_client_rect(data.window);
        });

        data.client_rect
    }

    #[inline]
    fn get_client_rect(&self, window: HWND) -> (u32, u32) {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(window, &mut rect);
        }

        (
            (rect.right - rect.left) as u32,
            (rect.bottom - rect.top) as u32,
        )
    }
}
