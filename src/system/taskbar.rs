// system/taskbar.rs — Position et état auto-hide de la barre des tâches.

use windows::{
    core::Result,
    Win32::{
        Foundation::RECT,
        UI::Shell::{
            SHAppBarMessage, APPBARDATA, ABM_GETAUTOHIDEBAR, ABM_GETTASKBARPOS,
            ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarEdge {
    Bottom,
    Top,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub struct TaskbarInfo {
    pub edge: TaskbarEdge,
    pub rect: RECT,
    pub auto_hide: bool,
}

impl TaskbarInfo {
    /// Coordonnée Y du bord supérieur (pour positionner les chats dessus si Bottom).
    pub fn cat_y_for_bottom(&self, cat_h: i32) -> i32 {
        match self.edge {
            TaskbarEdge::Bottom => {
                if self.auto_hide {
                    // Barre masquée : placer les chats au bord bas de l'écran.
                    let screen_h = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                            windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
                        )
                    };
                    screen_h - cat_h
                } else {
                    self.rect.top - cat_h
                }
            }
            TaskbarEdge::Top => self.rect.bottom,
            _ => self.rect.top,
        }
    }

    /// Coordonnée X min/max pour marcher sur le bord.
    pub fn walk_range_x(&self, cat_w: i32) -> (i32, i32) {
        (self.rect.left, self.rect.right - cat_w)
    }

    pub fn walk_range_y(&self, cat_h: i32) -> (i32, i32) {
        (self.rect.top, self.rect.bottom - cat_h)
    }
}

/// Interroge Windows pour la position et l'edge de la barre des tâches.
pub fn get_taskbar_info() -> Option<TaskbarInfo> {
    unsafe {
        let mut data = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            ..Default::default()
        };

        SHAppBarMessage(ABM_GETTASKBARPOS, &mut data);

        let edge = match data.uEdge {
            ABE_TOP => TaskbarEdge::Top,
            ABE_LEFT => TaskbarEdge::Left,
            ABE_RIGHT => TaskbarEdge::Right,
            _ => TaskbarEdge::Bottom,
        };

        // Test auto-hide
        let auto_hide = SHAppBarMessage(ABM_GETAUTOHIDEBAR, &mut data) != 0;

        Some(TaskbarInfo {
            edge,
            rect: data.rc,
            auto_hide,
        })
    }
}
