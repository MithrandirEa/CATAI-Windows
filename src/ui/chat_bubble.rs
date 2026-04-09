// ui/chat_bubble.rs — Fenêtre popup layered affichant les tokens Ollama.
// Fenêtre WS_EX_LAYERED sans bord, fond blanc semi-transparent, texte noir.
// Les tokens sont ajoutés au fil de l'eau ; la hauteur s'ajuste automatiquement.

use windows::{
    core::{Result, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, POINT, RECT, SIZE},
        Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC,
            DeleteObject, DrawTextW, FillRect, GetDC, ReleaseDC, SelectObject, SetBkMode,
            SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DT_LEFT,
            DT_TOP, DT_WORDBREAK, DIB_RGB_COLORS, FONT_CHARSET, FONT_CLIP_PRECISION,
            FONT_OUTPUT_PRECISION, FONT_QUALITY, FONT_WEIGHT,
            HGDIOBJ, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, ShowWindow, UpdateLayeredWindow,
            SW_HIDE, SW_SHOWNA, ULW_ALPHA, WINDOW_EX_STYLE, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST, WS_POPUP,
        },
    },
};

pub const BUBBLE_CLASS: PCWSTR = windows::core::w!("CATAI_Bubble");

const BUBBLE_W: i32 = 220;
const BUBBLE_PAD: i32 = 12;
const FONT_SIZE: i32 = 14;
const MAX_H: i32 = 300;

pub struct ChatBubble {
    pub hwnd: HWND,
    text: String,
}

impl ChatBubble {
    /// Crée la fenêtre (cachée par défaut).
    pub unsafe fn new() -> Result<Self> {
        let hinstance = HINSTANCE(GetModuleHandleW(None)?.0);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0),
            BUBBLE_CLASS,
            windows::core::w!(""),
            WS_POPUP,
            0, 0, BUBBLE_W, 60,
            None,
            None,
            Some(hinstance),
            None,
        )?
        ;
        Ok(Self { hwnd, text: String::new() })
    }

    /// Affiche ou met à jour la bulle au-dessus du chat.
    pub unsafe fn show(&mut self, cat_x: i32, cat_y: i32, cat_size: i32) -> Result<()> {
        let (x, y, w, h) = self.layout(cat_x, cat_y, cat_size);
        self.render(x, y, w, h)?;
        ShowWindow(self.hwnd, SW_SHOWNA);
        Ok(())
    }

    /// Ajoute des tokens et rafraîchit.
    pub unsafe fn append(&mut self, token: &str, cat_x: i32, cat_y: i32, cat_size: i32) -> Result<()> {
        self.text.push_str(token);
        self.show(cat_x, cat_y, cat_size)
    }

    /// Vide la bulle et la cache.
    pub unsafe fn hide_and_clear(&mut self) {
        self.text.clear();
        ShowWindow(self.hwnd, SW_HIDE);
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Drop for ChatBubble {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

impl ChatBubble {
    // ── Rendu ────────────────────────────────────────────────────────────────

    unsafe fn layout(&self, cat_x: i32, cat_y: i32, cat_size: i32) -> (i32, i32, i32, i32) {
        let w = BUBBLE_W;
        // Estimer la hauteur via DrawText avec DT_CALCRECT
        let h = self.measure_height(w).min(MAX_H).max(40);
        let x = cat_x - (w - cat_size) / 2;
        let y = cat_y - h - 8;
        (x, y, w, h)
    }

    unsafe fn measure_height(&self, w: i32) -> i32 {
        let h_screen = GetDC(None);
        let h_mem = CreateCompatibleDC(Some(h_screen));
        let hfont = make_font();
        let h_old = SelectObject(h_mem, HGDIOBJ(hfont.0));

        let mut rc = RECT {
            left: BUBBLE_PAD,
            top: BUBBLE_PAD,
            right: w - BUBBLE_PAD,
            bottom: 4096,
        };
        let mut text_w: Vec<u16> = self.text.encode_utf16().chain(std::iter::once(0)).collect();
        DrawTextW(
            h_mem,
            &mut text_w,
            &mut rc,
            DT_LEFT | DT_TOP | DT_WORDBREAK | windows::Win32::Graphics::Gdi::DT_CALCRECT,
        );

        let h = rc.bottom + BUBBLE_PAD * 2;
        SelectObject(h_mem, h_old);
        DeleteObject(HGDIOBJ(hfont.0));
        DeleteDC(h_mem);
        ReleaseDC(None, h_screen);
        h
    }

    unsafe fn render(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        let h_screen = GetDC(None);
        let h_mem = CreateCompatibleDC(Some(h_screen));

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pv_bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(Some(h_mem), &bmi, DIB_RGB_COLORS, &mut pv_bits, None, 0)?;
        let h_old_bmp = SelectObject(h_mem, HGDIOBJ(hbmp.0));

        // Fond blanc avec alpha 220
        let pixel_count = (w * h) as usize;
        let bits = std::slice::from_raw_parts_mut(pv_bits as *mut u32, pixel_count);
        let bg: u32 = (220 << 24) | (0xFA << 16) | (0xFA << 8) | 0xFA; // ARGB prémultiplié
        bits.fill(bg);

        // Texte noir
        let hfont = make_font();
        let h_old_font = SelectObject(h_mem, HGDIOBJ(hfont.0));
        SetBkMode(h_mem, TRANSPARENT);
        SetTextColor(h_mem, COLORREF(0x00000000));

        let mut rc = RECT {
            left: BUBBLE_PAD,
            top: BUBBLE_PAD,
            right: w - BUBBLE_PAD,
            bottom: h - BUBBLE_PAD,
        };
        let mut text_w: Vec<u16> = self.text.encode_utf16().chain(std::iter::once(0)).collect();
        DrawTextW(h_mem, &mut text_w, &mut rc, DT_LEFT | DT_TOP | DT_WORDBREAK);

        SelectObject(h_mem, h_old_font);
        DeleteObject(HGDIOBJ(hfont.0));

        // UpdateLayeredWindow
        let pt_src = POINT { x: 0, y: 0 };
        let pt_dst = POINT { x, y };
        let sz = SIZE { cx: w, cy: h };
        let blend = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1,
        };
        UpdateLayeredWindow(
            self.hwnd, Some(h_screen),
            Some(&pt_dst), Some(&sz),
            Some(h_mem), Some(&pt_src),
            COLORREF(0), Some(&blend), ULW_ALPHA,
        )?;

        SelectObject(h_mem, h_old_bmp);
        DeleteObject(HGDIOBJ(hbmp.0));
        DeleteDC(h_mem);
        ReleaseDC(None, h_screen);
        Ok(())
    }
}

unsafe fn make_font() -> windows::Win32::Graphics::Gdi::HFONT {
    CreateFontW(
        FONT_SIZE, 0, 0, 0,
        400, // FW_NORMAL
        0, 0, 0,
        FONT_CHARSET(1),  // DEFAULT_CHARSET
        FONT_OUTPUT_PRECISION(0), FONT_CLIP_PRECISION(0), FONT_QUALITY(0),
        0, // ipitchandfamily
        windows::core::w!("Segoe UI"),
    )
}
