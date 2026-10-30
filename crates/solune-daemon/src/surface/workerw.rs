//! Superfície WorkerW: o vídeo numa janela filha da área de trabalho do Windows.
//!
//! Uma mensagem ao Progman faz o Explorer criar um WorkerW atrás dos ícones;
//! a janela do vídeo é filha dele e fica sob os ícones e acima do papel de
//! parede. No Windows 11 24H2 o WorkerW é filho do Progman, e a janela vai
//! para o fundo da pilha dele. O libmpv desenha pela render API, com um
//! contexto WGL (OpenGL do próprio Windows).

use std::ffi::{c_char, c_void};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, mpsc};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetDC, HDC, ReleaseDC};
use windows_sys::Win32::Graphics::OpenGL::{
    ChoosePixelFormat, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA,
    PIXELFORMATDESCRIPTOR, SetPixelFormat, SwapBuffers, wglCreateContext, wglDeleteContext,
    wglGetProcAddress, wglMakeCurrent,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleA, GetModuleHandleW, GetProcAddress,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowExW,
    FindWindowW, GetClientRect, GetMessageW, GetSystemMetrics, HWND_BOTTOM, IsWindow, KillTimer,
    MSG, MoveWindow, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, RegisterClassExW,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SMTO_NORMAL, SPI_SETDESKWALLPAPER, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SendMessageTimeoutW, SetProcessDPIAware, SetTimer, SetWindowPos,
    SystemParametersInfoW, TranslateMessage, WM_APP, WM_ERASEBKGND, WM_TIMER, WNDCLASSEXW,
    WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};
use windows_sys::w;

use super::{Control, Ready};
use crate::mpv::Mpv;
use crate::render::{Interop, RenderContext, Wakeup};

/// Acorda o laço de mensagens da thread: libmpv com quadro novo ou pedido do player.
const WM_WAKE: u32 = WM_APP;

/// Mensagem que faz o Explorer criar o WorkerW atrás dos ícones.
const SPAWN_WORKERW: u32 = 0x052C;

/// Intervalo da conferência do tamanho da tela e do Explorer.
const WATCH_MS: u32 = 2000;
const WATCH_TIMER: usize = 1;

const SPIF_UPDATEINIFILE: u32 = 1;

fn wake(thread: u32) {
    if thread != 0 {
        // SAFETY: só põe uma mensagem na fila da thread; falha se ela já saiu.
        unsafe { PostThreadMessageW(thread, WM_WAKE, 0, 0) };
    }
}

/// Lado do player do canal de pedidos: manda o pedido e acorda a thread.
#[derive(Debug)]
pub(super) struct Sender {
    tx: mpsc::Sender<Control>,
    thread: Arc<AtomicU32>,
}

impl Sender {
    pub(super) fn send(&self, control: Control) -> Result<(), ()> {
        self.tx.send(control).map_err(|_| ())?;
        wake(self.thread.load(Ordering::Acquire));
        Ok(())
    }
}

pub(super) struct Receiver {
    rx: mpsc::Receiver<Control>,
    thread: Arc<AtomicU32>,
}

pub(super) fn channel() -> (Sender, Receiver) {
    let (tx, rx) = mpsc::channel();
    let thread = Arc::new(AtomicU32::new(0));
    (
        Sender {
            tx,
            thread: Arc::clone(&thread),
        },
        Receiver { rx, thread },
    )
}

/// Acorda o laço quando o libmpv tem um quadro novo.
struct Waker(u32);

impl Wakeup for Waker {
    fn wake(&self) {
        wake(self.0);
    }
}

/// Endereços das funções do OpenGL para o libmpv: as do driver pelo WGL e as
/// do OpenGL 1.1 direto do opengl32.dll, que o WGL não devolve.
unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: o libmpv passa um nome terminado em nulo; o contexto WGL é atual.
    unsafe {
        if let Some(f) = wglGetProcAddress(name.cast()) {
            let address = f as usize;
            // Alguns drivers devolvem 1, 2, 3 ou -1 em vez de nulo.
            if address > 3 && address != usize::MAX {
                return address as *mut c_void;
            }
        }
        let module = GetModuleHandleA(c"opengl32.dll".as_ptr().cast());
        GetProcAddress(module, name.cast()).map_or(null_mut(), |f| f as *mut c_void)
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // O vídeo cobre tudo: sem apagar o fundo, não pisca entre os quadros.
    if msg == WM_ERASEBKGND {
        return 1;
    }
    // SAFETY: repassa os argumentos que o Windows deu.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe extern "system" fn find_workerw(top: HWND, out: LPARAM) -> i32 {
    // SAFETY: `out` é o `*mut HWND` de `find_parent`, vivo durante o EnumWindows.
    unsafe {
        if !FindWindowExW(top, null_mut(), w!("SHELLDLL_DefView"), null()).is_null() {
            // O WorkerW logo depois do que guarda os ícones é o de trás.
            *(out as *mut HWND) = FindWindowExW(null_mut(), top, w!("WorkerW"), null());
            return 0;
        }
    }
    1
}

/// A janela pai do vídeo e se ela é o Progman (Windows 11 24H2 em diante).
fn find_parent() -> Result<(HWND, bool), String> {
    // SAFETY: chamadas de janela sem ponteiros além dos locais abaixo.
    unsafe {
        let progman = FindWindowW(w!("Progman"), null());
        if progman.is_null() {
            return Err("o Explorer não está rodando (sem Progman)".into());
        }
        let mut result = 0;
        SendMessageTimeoutW(
            progman,
            SPAWN_WORKERW,
            0xD,
            1,
            SMTO_NORMAL,
            1000,
            &mut result,
        );
        let mut workerw: HWND = null_mut();
        EnumWindows(Some(find_workerw), (&raw mut workerw) as LPARAM);
        if !workerw.is_null() {
            return Ok((workerw, false));
        }
        // 24H2 e 25H2: o WorkerW do papel de parede é filho do Progman, e a
        // janela vai para o Progman, logo abaixo dos ícones.
        Ok((progman, true))
    }
}

fn client_size(parent: HWND) -> (i32, i32) {
    // SAFETY: parent é uma janela; o Windows preenche o RECT.
    let rect = unsafe {
        let mut rect: RECT = zeroed();
        GetClientRect(parent, &mut rect);
        rect
    };
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    if w > 0 && h > 0 {
        return (w, h);
    }
    // SAFETY: só lê métricas do sistema.
    unsafe {
        (
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        )
    }
}

/// A janela do vídeo com seu contexto OpenGL. Solta tudo ao sair.
struct Window {
    parent: HWND,
    hwnd: HWND,
    hdc: HDC,
    context: *mut c_void,
}

impl Window {
    fn open(parent: HWND, bottom: bool) -> Result<Self, String> {
        let (width, height) = client_size(parent);
        // SAFETY: parent é uma janela do Explorer; a classe e a janela são
        // desta thread e saem no Drop.
        unsafe {
            let instance = GetModuleHandleW(null());
            let mut class: WNDCLASSEXW = zeroed();
            class.cbSize = size_of::<WNDCLASSEXW>() as u32;
            class.lpfnWndProc = Some(window_proc);
            class.hInstance = instance;
            class.lpszClassName = w!("SoluneWallpaper");
            // Falha se a classe já existe (um segundo start): é a mesma, serve.
            RegisterClassExW(&class);
            let hwnd = CreateWindowExW(
                0,
                w!("SoluneWallpaper"),
                null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                0,
                0,
                width,
                height,
                parent,
                null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                return Err("não foi possível criar a janela do papel de parede".into());
            }
            let mut window = Self {
                parent,
                hwnd,
                hdc: GetDC(hwnd),
                context: null_mut(),
            };
            if bottom {
                SetWindowPos(
                    hwnd,
                    HWND_BOTTOM,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            let mut pfd: PIXELFORMATDESCRIPTOR = zeroed();
            pfd.nSize = size_of::<PIXELFORMATDESCRIPTOR>() as u16;
            pfd.nVersion = 1;
            pfd.dwFlags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
            pfd.iPixelType = PFD_TYPE_RGBA;
            pfd.cColorBits = 24;
            let format = ChoosePixelFormat(window.hdc, &pfd);
            if format == 0 || SetPixelFormat(window.hdc, format, &pfd) == 0 {
                return Err("nenhum formato de pixel OpenGL para a janela".into());
            }
            window.context = wglCreateContext(window.hdc);
            if window.context.is_null() || wglMakeCurrent(window.hdc, window.context) == 0 {
                return Err("não foi possível criar o contexto OpenGL (WGL)".into());
            }
            Ok(window)
        }
    }

    /// Ajusta ao tamanho do pai; `Some` com o novo tamanho se mudou.
    fn fit(&self, size: (i32, i32)) -> Option<(i32, i32)> {
        let now = client_size(self.parent);
        if now == size {
            return None;
        }
        // SAFETY: hwnd é a janela desta thread.
        unsafe { MoveWindow(self.hwnd, 0, 0, now.0, now.1, 1) };
        Some(now)
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: tudo isto foi criado por esta thread e é solto uma única vez.
        unsafe {
            wglMakeCurrent(null_mut(), null_mut());
            if !self.context.is_null() {
                wglDeleteContext(self.context);
            }
            ReleaseDC(self.hwnd, self.hdc);
            DestroyWindow(self.hwnd);
            // O Explorer pinta o papel de parede de novo.
            SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, null_mut(), SPIF_UPDATEINIFILE);
        }
    }
}

fn new_render(player: &Arc<Mpv>, thread: u32) -> Result<RenderContext<Waker>, String> {
    // SAFETY: o contexto WGL é atual nesta thread desde `Window::open`.
    unsafe {
        RenderContext::new(
            Arc::clone(player),
            get_proc_address,
            Interop::None,
            Waker(thread),
        )
    }
    .map_err(|e| e.to_string())
}

/// Roda a superfície até `control` pedir para parar ou o Explorer sumir.
pub(super) fn run(player: Arc<Mpv>, ready: Ready, control: Receiver) -> Result<(), String> {
    // Sem isto o Windows estica a janela em telas com escala diferente de 100%.
    // SAFETY: sem argumentos; vale para o processo todo.
    unsafe { SetProcessDPIAware() };

    // SAFETY: cria a fila de mensagens da thread antes de publicar o id dela.
    let thread = unsafe {
        let mut msg: MSG = zeroed();
        PeekMessageW(&mut msg, null_mut(), WM_WAKE, WM_WAKE, PM_NOREMOVE);
        GetCurrentThreadId()
    };
    control.thread.store(thread, Ordering::Release);

    let (parent, bottom) = find_parent()?;
    let window = Window::open(parent, bottom)?;
    let mut size = client_size(parent);
    tracing::info!("janela do WorkerW: {}x{}", size.0, size.1);
    // Declarado depois da janela: sai antes dela, com o contexto ainda atual.
    let mut render = Some(new_render(&player, thread)?);
    let mut frozen = false;
    // SAFETY: timer de thread sem janela (hwnd nulo), solto abaixo.
    let timer = unsafe { SetTimer(null_mut(), WATCH_TIMER, WATCH_MS, None) };
    ready.ok();

    let mut result = Ok(());
    'events: loop {
        // SAFETY: o MSG vive durante a chamada.
        let mut msg: MSG = unsafe { zeroed() };
        // SAFETY: msg é válido; lê a fila desta thread.
        if unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } <= 0 {
            break;
        }
        match msg.message {
            WM_WAKE => {
                while let Ok(request) = control.rx.try_recv() {
                    match request {
                        Control::Stop => break 'events,
                        Control::Freeze(reply) => {
                            let ok = render.as_ref().map(|_| ()).ok_or("sem render do libmpv");
                            frozen |= ok.is_ok();
                            let _ = reply.send(ok.map_err(str::to_owned));
                        }
                        Control::Release(reply) => {
                            let ok = if frozen {
                                render = None;
                                Ok(())
                            } else {
                                Err("o render precisa estar congelado".into())
                            };
                            let _ = reply.send(ok);
                        }
                        Control::Thaw(reply) => {
                            frozen = false;
                            let ok = match render {
                                Some(_) => Ok(()),
                                None => new_render(&player, thread).map(|r| render = Some(r)),
                            };
                            let _ = reply.send(ok);
                        }
                    }
                }
                if let Some(render) = render.as_ref().filter(|_| !frozen)
                    && render.needs_frame()
                    && let Err(e) = draw(render, &window, size)
                {
                    result = Err(e);
                    break;
                }
            }
            WM_TIMER => {
                // SAFETY: parent é uma janela do Explorer ou já morreu.
                if unsafe { IsWindow(parent) } == 0 {
                    result = Err("o Explorer reiniciou".into());
                    break;
                }
                if let Some(now) = window.fit(size) {
                    tracing::info!("resolução da tela mudou: {}x{}", now.0, now.1);
                    size = now;
                    // Sem quadro novo, o libmpv desenha o último de novo.
                    if let Some(render) = render.as_ref().filter(|_| !frozen)
                        && let Err(e) = draw(render, &window, size)
                    {
                        result = Err(e);
                        break;
                    }
                }
            }
            _ => {
                // SAFETY: msg foi preenchida pelo GetMessageW.
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
    // SAFETY: o timer é desta thread.
    unsafe { KillTimer(null_mut(), timer) };
    drop(render);
    result
}

fn draw(render: &RenderContext<Waker>, window: &Window, size: (i32, i32)) -> Result<(), String> {
    render
        .render(0, size.0, size.1)
        .map_err(|e| e.to_string())?;
    // SAFETY: hdc é o da janela, com o contexto atual.
    unsafe { SwapBuffers(window.hdc) };
    render.report_swap();
    Ok(())
}
