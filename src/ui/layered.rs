// ui/layered.rs — Fenêtres transparentes pixel-par-pixel via UpdateLayeredWindow.
// Rendu exclusivement via UpdateLayeredWindow + DIB BGRA prémultiplié.
// Jamais de WM_PAINT pour ces fenêtres.

use windows::{
    core::Result,
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, POINT, SIZE},
        Graphics::Gdi::{
            BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
            GetDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
            BLENDFUNCTION, DIB_RGB_COLORS, HDC, HGDIOBJ,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, SetLayeredWindowAttributes, ShowWindow, UpdateLayeredWindow,
            LWA_ALPHA, SW_SHOWNA, ULW_ALPHA, WINDOW_EX_STYLE, WINDOW_STYLE,
            WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
        System::LibraryLoader::GetModuleHandleW,
    },
};

pub const CAT_CLASS: windows::core::PCWSTR = windows::core::w!("CATAI_Cat");

/// Crée une fenêtre layered pour un chat.
/// Style : WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW + WS_POPUP
/// Pas de WS_EX_APPWINDOW → invisible dans la barre des tâches.
pub unsafe fn create_cat_window(x: i32, y: i32, w: i32, h: i32) -> Result<HWND> {
    let hinstance = HINSTANCE(GetModuleHandleW(None)?.0);

    let ex_style = WINDOW_EX_STYLE(
        WS_EX_LAYERED.0 | WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0,
    );

    let hwnd = CreateWindowExW(
        ex_style,
        CAT_CLASS,
        windows::core::w!(""),
        WS_POPUP,
        x,
        y,
        w,
        h,
        None,
        None,
        Some(hinstance),
        None,
    )?;

    ShowWindow(hwnd, SW_SHOWNA);
    Ok(hwnd)
}

/// Met à jour le contenu visuel d'une fenêtre layered.
/// `bgra` : pixels BGRA prémultipliés, `w * h * 4` octets.
/// `screen_x / screen_y` : position absolue souhaitée pour la fenêtre.
pub unsafe fn update_layered(
    hwnd: HWND,
    bgra: &[u8],
    w: u32,
    h: u32,
    screen_x: i32,
    screen_y: i32,
) -> Result<()> {
    let h_screen = GetDC(None);
    let h_mem = CreateCompatibleDC(Some(h_screen));

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32), // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        ..Default::default()
    };

    let mut pv_bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let hbmp = CreateDIBSection(Some(h_mem), &bmi, DIB_RGB_COLORS, &mut pv_bits, None, 0)?;;

    // Copie des pixels dans le DIB
    let dst = std::slice::from_raw_parts_mut(pv_bits as *mut u8, (w * h * 4) as usize);
    dst.copy_from_slice(bgra);

    let h_old = SelectObject(h_mem, HGDIOBJ(hbmp.0));

    let pt_src = POINT { x: 0, y: 0 };
    let pt_dst = POINT {
        x: screen_x,
        y: screen_y,
    };
    let sz = SIZE {
        cx: w as i32,
        cy: h as i32,
    };
    let blend = BLENDFUNCTION {
        BlendOp: 0,       // AC_SRC_OVER
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: 1,   // AC_SRC_ALPHA
    };

    UpdateLayeredWindow(hwnd, Some(h_screen), Some(&pt_dst), Some(&sz), Some(h_mem), Some(&pt_src), COLORREF(0), Some(&blend), ULW_ALPHA)?;

    SelectObject(h_mem, h_old);
    DeleteObject(HGDIOBJ(hbmp.0));
    DeleteDC(h_mem);
    ReleaseDC(None, h_screen);

    Ok(())
}
