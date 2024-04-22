#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{anyhow, Result};
use std::mem::size_of;
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateFontIndirectW, CreatePen, CreateSolidBrush, DeleteObject, EndPaint,
            FillRect, GetStockObject, InvalidateRect, LineTo, MoveToEx, PtInRect, Rectangle,
            ScreenToClient, SelectObject, DT_SINGLELINE, DT_VCENTER, DT_WORD_ELLIPSIS, HFONT,
            HOLLOW_BRUSH, HPEN, LOGFONTW, PAINTSTRUCT, PS_SOLID,
        },
        UI::{
            Controls::{
                CloseThemeData, DrawThemeTextEx, GetThemePartSize, OpenThemeData, CS_ACTIVE,
                DTTOPTS, DTT_TEXTCOLOR, TS_TRUE, WP_CAPTION,
            },
            HiDpi::{
                GetDpiForWindow, GetSystemMetricsForDpi, SetProcessDpiAwarenessContext,
                SystemParametersInfoForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            },
            Input::KeyboardAndMouse::GetFocus,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetCursorPos,
                GetMessageW, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, LoadCursorW, PostMessageW, PostQuitMessage,
                RegisterClassExW, SetCursor, SetWindowLongPtrW, SetWindowPos, ShowWindow,
                TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, HTBOTTOM,
                HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTMAXBUTTON, HTNOWHERE,
                HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, MSG, NCCALCSIZE_PARAMS,
                SHOW_WINDOW_CMD, SM_CXFRAME, SM_CXPADDEDBORDER, SM_CYFRAME,
                SPI_GETICONTITLELOGFONT, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SW_MAXIMIZE,
                SW_MINIMIZE, SW_NORMAL, SW_SHOWMAXIMIZED, WINDOWPLACEMENT, WM_ACTIVATE, WM_CLOSE,
                WM_CREATE, WM_DESTROY, WM_MOUSEMOVE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDOWN,
                WM_NCLBUTTONUP, WM_NCMOUSEMOVE, WM_PAINT, WM_SETCURSOR, WNDCLASSEXW,
                WS_EX_APPWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
                WS_VISIBLE,
            },
        },
    },
};

fn main() -> Result<()> {
    if let Err(e) =
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
    {
        return Err(anyhow!("Failed to set DPI awareness: {}", e.message()));
    };

    let window_class_name = w!("Tremind Window Class");
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpszClassName: window_class_name,
        lpfnWndProc: Some(window_proc),
        style: CS_HREDRAW | CS_VREDRAW,
        ..Default::default()
    };

    unsafe { RegisterClassExW(&window_class) };

    let window_style = WS_THICKFRAME | WS_SYSMENU | WS_MAXIMIZEBOX | WS_MINIMIZEBOX | WS_VISIBLE;

    unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            window_class_name,
            w!("Tremind"),
            window_style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            800,
            600,
            None,
            None,
            None,
            None,
        )
    };

    let mut message: MSG = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0).0 > 0 } {
        unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }

    Ok(())
}

const DEFAULT_DPI: f32 = 96.0;
fn win32_dpi_scale(value: i32, dpi: u32) -> i32 {
    (value as f32 * dpi as f32 / DEFAULT_DPI) as i32
}

// 1 pixel border on top and 1 on bottom
const TOP_N_BOTTOM_BORDERS_SIZE: i32 = 2;
fn win32_titlebar_rect(handle: HWND) -> Result<RECT> {
    let theme = unsafe { OpenThemeData(handle, w!("WINDOW")) };
    let dpi = unsafe { GetDpiForWindow(handle) };
    let titlebar_size = unsafe {
        GetThemePartSize(theme, None, WP_CAPTION.0, CS_ACTIVE.0, None, TS_TRUE)
            .map_err(|e| anyhow!("{}", e.message()))?
    };

    unsafe { CloseThemeData(theme).map_err(|e| anyhow!("{}", e.message()))? };

    let height = win32_dpi_scale(titlebar_size.cy, dpi) + TOP_N_BOTTOM_BORDERS_SIZE;
    let mut rect = RECT::default();

    unsafe { GetClientRect(handle, &mut rect).map_err(|e| anyhow!("{}", e.message()))? };

    rect.bottom = rect.top + height;
    Ok(rect)
}

// Set this to 0 to remove the fake shadow painting
const WIN32_FAKE_SHADOW_HEIGHT: i32 = 1;
// The offset of the 2 rectangles of the maximized window button
const WIN32_MAXIMIZED_BUTTON_OFFSET: i32 = 2;

fn win32_fake_shadow_rect(handle: HWND) -> Result<RECT> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(handle, &mut rect).map_err(|e| anyhow!("{}", e.message()))? };
    rect.bottom = rect.top + WIN32_FAKE_SHADOW_HEIGHT;
    Ok(rect)
}

struct CustomTitleBarButtonRects {
    close: RECT,
    maximize: RECT,
    minimize: RECT,
}

#[derive(PartialEq)]
enum CustomTitleBarHoveredButton {
    None,
    Minimize,
    Maximize,
    Close,
}

impl From<isize> for CustomTitleBarHoveredButton {
    fn from(item: isize) -> Self {
        match item {
            1 => Self::Minimize,
            2 => Self::Maximize,
            3 => Self::Close,
            _ => Self::None,
        }
    }
}

impl CustomTitleBarButtonRects {
    fn win32_get_title_bar_button_rects(handle: HWND, title_bar_rect: &RECT) -> Self {
        let dpi = unsafe { GetDpiForWindow(handle) };
        let button_width = win32_dpi_scale(47, dpi);

        // modify original c code a bit to make it more idiomatic
        let close = RECT {
            top: title_bar_rect.top + WIN32_FAKE_SHADOW_HEIGHT,
            left: title_bar_rect.right - button_width,
            ..*title_bar_rect
        };

        let maximize = RECT {
            left: close.left - button_width,
            right: close.right - button_width,
            ..close
        };

        let minimize = RECT {
            left: maximize.left - button_width,
            right: maximize.right - button_width,
            ..maximize
        };

        Self {
            close,
            maximize,
            minimize,
        }
    }
}

fn win32_window_is_maximized(handle: HWND) -> Result<bool> {
    let mut placement = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(handle, &mut placement).map_err(|e| anyhow!("{}", e.message()))? };
    Ok(SHOW_WINDOW_CMD(placement.showCmd as _) == SW_SHOWMAXIMIZED)
}

// I think this is for centering icon in the title bar's buttons
// to center = the rect to center
// outer_rect = the button rect
fn win32_center_rect_in_rect(to_center: &mut RECT, outer_rect: &RECT) {
    let to_width = to_center.right - to_center.left;
    let to_height = to_center.bottom - to_center.top;
    let outer_width = outer_rect.right - outer_rect.left;
    let outer_height = outer_rect.bottom - outer_rect.top;

    let padding_x = (outer_width - to_width) / 2;
    let padding_y = (outer_height - to_height) / 2;

    to_center.left = outer_rect.left + padding_x;
    to_center.top = outer_rect.top + padding_y;
    to_center.right = to_center.left + to_width;
    to_center.bottom = to_center.top + to_height;
}

// Description:
// 0xffff = 65535, so we take the first 16 bits
// We need to cast to i16 first in order to maintain the sign (negative or positive) then cast to i32
const fn get_x_param(l_param: LPARAM) -> i32 {
    (l_param.0 & 0xffff) as i16 as i32
}

// 0xffff0000 = 4294901760, so we take the last 16 bits.
// The last 0000 is as we >> aka shift right by 16
const fn get_y_param(l_param: LPARAM) -> i32 {
    ((l_param.0 >> 16) & 0xffff) as i16 as i32
}

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

const fn get_r_value(rgb: u32) -> u8 {
    (rgb & 0xff) as u8
}

const fn get_g_value(rgb: u32) -> u8 {
    ((rgb >> 8) & 0xff) as u8
}

const fn get_b_value(rgb: u32) -> u8 {
    ((rgb >> 16) & 0xff) as u8
}

#[allow(clippy::cognitive_complexity)]
unsafe extern "system" fn window_proc(
    handle: HWND,
    message: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let title_bar_hovered_button: CustomTitleBarHoveredButton =
        GetWindowLongPtrW(handle, GWLP_USERDATA).into();

    match message {
        WM_NCCALCSIZE => {
            if w_param == WPARAM(0) {
                return DefWindowProcW(handle, message, w_param, l_param);
            }

            let dpi = GetDpiForWindow(handle);
            let frame_x = GetSystemMetricsForDpi(SM_CXFRAME, dpi);
            let frame_y = GetSystemMetricsForDpi(SM_CYFRAME, dpi);
            let padding = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);

            let params = l_param.0 as *mut NCCALCSIZE_PARAMS;
            if params.is_null() {
                return DefWindowProcW(handle, message, w_param, l_param);
            }

            let requested_client_rect = &mut (*params).rgrc[0];
            requested_client_rect.right -= frame_x + padding;
            requested_client_rect.left += frame_x + padding;
            requested_client_rect.bottom -= frame_y + padding;

            let is_maximized = win32_window_is_maximized(handle);
            if matches!(is_maximized, Ok(true)) {
                requested_client_rect.top += padding;
            } else if let Err(e) = is_maximized {
                eprintln!("Failed to get window maximized state\n{:?}", e);
            }

            return LRESULT(0);
        }
        WM_CREATE => {
            let mut size_rect = RECT::default();
            let result = GetWindowRect(handle, &mut size_rect);

            if result.is_err() {
                eprintln!(
                    "Failed to get window rect:\n{}",
                    result.err().unwrap().message()
                );
                return DefWindowProcW(handle, message, w_param, l_param);
            }

            let result = SetWindowPos(
                handle,
                None,
                size_rect.left,
                size_rect.top,
                size_rect.right - size_rect.left,
                size_rect.bottom - size_rect.top,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE,
            );

            if result.is_err() {
                eprintln!(
                    "Failed to set window position:\n{}",
                    result.err().unwrap().message()
                );
                return DefWindowProcW(handle, message, w_param, l_param);
            }
        }
        WM_ACTIVATE => {
            let result = win32_titlebar_rect(handle);
            if result.is_err() {
                eprintln!("Failed to get title bar rect:\n{}", result.err().unwrap());
                return DefWindowProcW(handle, message, w_param, l_param);
            }

            let title_bar_rect = result.unwrap();
            InvalidateRect(handle, Some(&title_bar_rect), false);

            return DefWindowProcW(handle, message, w_param, l_param);
        }
        WM_NCHITTEST => {
            let hit = DefWindowProcW(handle, message, w_param, l_param);
            match hit.0 as u32 {
                HTNOWHERE | HTRIGHT | HTLEFT | HTTOPLEFT | HTTOP | HTTOPRIGHT | HTBOTTOMRIGHT
                | HTBOTTOM | HTBOTTOMLEFT => {
                    return hit;
                }
                _ => {}
            }

            if title_bar_hovered_button == CustomTitleBarHoveredButton::Maximize {
                return LRESULT(HTMAXBUTTON as _);
            }

            let dpi = GetDpiForWindow(handle);
            let frame_y = GetSystemMetricsForDpi(SM_CYFRAME, dpi);
            let padding = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
            let mut cursor_point = POINT {
                x: get_x_param(l_param),
                y: get_y_param(l_param),
            };

            ScreenToClient(handle, &mut cursor_point);

            if cursor_point.y > 0 && cursor_point.y < frame_y + padding {
                return LRESULT(HTTOP as _);
            }

            let result = win32_titlebar_rect(handle);
            if result.is_err() {
                eprintln!("Failed to get title bar rect:\n{}", result.err().unwrap());
                return hit;
            }

            let title_bar_rect = result.unwrap();

            if cursor_point.y < title_bar_rect.bottom {
                return LRESULT(HTCAPTION as _);
            }

            return LRESULT(HTCLIENT as _);
        }
        WM_PAINT => {
            let has_focus = GetFocus() == handle;
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(handle, &mut ps);

            // Paint background
            let bg_color = COLORREF(rgb(200, 250, 230));
            let bg_brush = CreateSolidBrush(bg_color);
            FillRect(hdc, &ps.rcPaint, bg_brush);
            DeleteObject(bg_brush);

            // Paint title bar
            let theme = OpenThemeData(handle, w!("WINDOW"));

            let titlebar_color = COLORREF(rgb(150, 200, 180));
            let titlebar_brush = CreateSolidBrush(titlebar_color);
            let titlebar_hover_color = COLORREF(rgb(130, 180, 160));
            let titlebar_hover_brush = CreateSolidBrush(titlebar_hover_color);

            let result = win32_titlebar_rect(handle);
            if result.is_err() {
                eprintln!("Failed to get title bar rect:\n{}", result.err().unwrap());
                return DefWindowProcW(handle, message, w_param, l_param);
            }

            let title_bar_rect = result.unwrap();

            // Title Bar Background
            FillRect(hdc, &title_bar_rect, titlebar_brush);

            let titlebar_item_color = COLORREF(if has_focus {
                rgb(33, 33, 33)
            } else {
                rgb(127, 127, 127)
            });

            let button_icon_brush = CreateSolidBrush(titlebar_item_color);
            let button_icon_pen = CreatePen(PS_SOLID, 1, titlebar_item_color);

            let button_rects = CustomTitleBarButtonRects::win32_get_title_bar_button_rects(
                handle,
                &title_bar_rect,
            );

            let dpi = GetDpiForWindow(handle);
            let icon_dimension = win32_dpi_scale(10, dpi);

            // Minimize Button
            {
                if title_bar_hovered_button == CustomTitleBarHoveredButton::Minimize {
                    FillRect(hdc, &button_rects.minimize, titlebar_hover_brush);
                }
                let mut icon_rect = RECT {
                    right: icon_dimension,
                    bottom: 1,
                    ..Default::default()
                };

                win32_center_rect_in_rect(&mut icon_rect, &button_rects.minimize);
                FillRect(hdc, &icon_rect, button_icon_brush);
            }

            // Maximize Button
            {
                let is_hovered =
                    if title_bar_hovered_button == CustomTitleBarHoveredButton::Maximize {
                        FillRect(hdc, &button_rects.maximize, titlebar_hover_brush);
                        true
                    } else {
                        false
                    };

                let mut icon_rect = RECT {
                    right: icon_dimension,
                    bottom: icon_dimension,
                    ..Default::default()
                };

                win32_center_rect_in_rect(&mut icon_rect, &button_rects.maximize);
                SelectObject(hdc, button_icon_pen);
                SelectObject(hdc, GetStockObject(HOLLOW_BRUSH));
                if matches!(win32_window_is_maximized(handle), Ok(true)) {
                    Rectangle(
                        hdc,
                        icon_rect.left + WIN32_MAXIMIZED_BUTTON_OFFSET,
                        icon_rect.top - WIN32_MAXIMIZED_BUTTON_OFFSET,
                        icon_rect.right + WIN32_MAXIMIZED_BUTTON_OFFSET,
                        icon_rect.bottom - WIN32_MAXIMIZED_BUTTON_OFFSET,
                    );

                    FillRect(
                        hdc,
                        &icon_rect,
                        if is_hovered {
                            titlebar_hover_brush
                        } else {
                            titlebar_brush
                        },
                    );
                }

                Rectangle(
                    hdc,
                    icon_rect.left,
                    icon_rect.top,
                    icon_rect.right,
                    icon_rect.bottom,
                );
            }

            // Close button
            {
                let mut custom_pen = HPEN(0);
                if title_bar_hovered_button == CustomTitleBarHoveredButton::Close {
                    let fill_brush = CreateSolidBrush(COLORREF(rgb(255, 0, 0))); // aka red color!!
                    FillRect(hdc, &button_rects.close, fill_brush);
                    DeleteObject(fill_brush);
                    custom_pen = CreatePen(PS_SOLID, 1, COLORREF(rgb(255, 255, 255)));
                    SelectObject(hdc, custom_pen);
                }

                let mut icon_rect = RECT {
                    right: icon_dimension,
                    bottom: icon_dimension,
                    ..Default::default()
                };

                win32_center_rect_in_rect(&mut icon_rect, &button_rects.close);
                MoveToEx(hdc, icon_rect.left, icon_rect.top, None);
                LineTo(hdc, icon_rect.right + 1, icon_rect.bottom + 1);
                MoveToEx(hdc, icon_rect.left, icon_rect.bottom, None);
                LineTo(hdc, icon_rect.right + 1, icon_rect.top - 1);
                if custom_pen != HPEN(0) {
                    DeleteObject(custom_pen);
                }
            }

            DeleteObject(titlebar_hover_brush);
            DeleteObject(button_icon_brush);
            DeleteObject(button_icon_pen);
            DeleteObject(titlebar_brush);

            // Draw window title
            let mut logical_font = LOGFONTW::default();
            let old_font = if SystemParametersInfoForDpi(
                SPI_GETICONTITLELOGFONT.0,
                size_of::<LOGFONTW>() as _,
                Some(&mut logical_font as *mut LOGFONTW as _),
                0,
                dpi,
            )
            .is_ok()
            {
                let theme_font = CreateFontIndirectW(&logical_font);
                HFONT(SelectObject(hdc, theme_font).0)
            } else {
                HFONT(0)
            };

            // Get title in title bar
            let text_length = GetWindowTextLengthW(handle);
            let mut title_text_buffer = vec![0u16; text_length as usize + 1];
            GetWindowTextW(handle, &mut title_text_buffer);
            // let mut titlebar_text_rect = title_bar_rect;

            // add padding to the left (title) and right (buttons)
            let text_padding = 10;
            let mut titlebar_text_rect = RECT {
                left: title_bar_rect.left + text_padding,
                right: button_rects.minimize.left - text_padding,
                ..title_bar_rect
            };

            let draw_theme_options = DTTOPTS {
                dwSize: size_of::<DTTOPTS>() as u32,
                dwFlags: DTT_TEXTCOLOR,
                crText: titlebar_item_color,
                ..Default::default()
            };

            // Draw title text
            if let Err(e) = DrawThemeTextEx(
                theme,
                hdc,
                WP_CAPTION.0,
                CS_ACTIVE.0,
                &title_text_buffer,
                DT_VCENTER | DT_SINGLELINE | DT_WORD_ELLIPSIS,
                &mut titlebar_text_rect,
                Some(&draw_theme_options),
            ) {
                eprintln!("Failed to draw theme text: {}", e.message());
            };

            if old_font != HFONT(0) {
                SelectObject(hdc, old_font);
            }

            if let Err(e) = CloseThemeData(theme) {
                eprintln!("Failed to close theme data: {}", e.message());
            };

            // Paint fake top shadow. Original is missing because of the client rect extension.
            // You might need to tweak the colors here based on the color scheme of your app
            // or just remove it if you decide it is not worth it.
            let shadow_color = COLORREF(rgb(100, 100, 100));
            let fake_top_shadow_color = if has_focus {
                shadow_color
            } else {
                let titlebar_color_value = titlebar_color.0;
                let shadow_color_value = shadow_color.0;
                COLORREF(rgb(
                    ((get_r_value(titlebar_color_value) as u32
                        + get_r_value(shadow_color_value) as u32)
                        / 2) as u8,
                    ((get_g_value(titlebar_color_value) as u32
                        + get_g_value(shadow_color_value) as u32)
                        / 2) as u8,
                    ((get_b_value(titlebar_color_value) as u32
                        + get_b_value(shadow_color_value) as u32)
                        / 2) as u8,
                ))
            };

            let fake_top_shadow_brush = CreateSolidBrush(fake_top_shadow_color);
            let result = win32_fake_shadow_rect(handle);
            if result.is_err() {
                eprintln!("Failed to get fake shadow rect:\n{}", result.err().unwrap());
                return DefWindowProcW(handle, message, w_param, l_param);
            }

            let fake_top_shadow_rect = result.unwrap();
            FillRect(hdc, &fake_top_shadow_rect, fake_top_shadow_brush);
            DeleteObject(fake_top_shadow_brush);

            EndPaint(handle, &ps);
        }
        // Track when mouse hovers each of the title bar buttons to draw the highlight correctly
        WM_NCMOUSEMOVE => {
            let mut cursor_point = POINT::default();
            if let Err(e) = GetCursorPos(&mut cursor_point) {
                eprintln!("Failed to get cursor position: {}", e.message());
                return DefWindowProcW(handle, message, w_param, l_param);
            };

            ScreenToClient(handle, &mut cursor_point);

            let result = win32_titlebar_rect(handle);
            if result.is_err() {
                eprintln!("Failed to get title bar rect:\n{}", result.err().unwrap());
                return DefWindowProcW(handle, message, w_param, l_param);
            }

            let title_bar_rect = result.unwrap();
            let button_rects = CustomTitleBarButtonRects::win32_get_title_bar_button_rects(
                handle,
                &title_bar_rect,
            );
            let new_hovered_button = if PtInRect(&button_rects.minimize, cursor_point).as_bool() {
                CustomTitleBarHoveredButton::Minimize
            } else if PtInRect(&button_rects.maximize, cursor_point).as_bool() {
                CustomTitleBarHoveredButton::Maximize
            } else if PtInRect(&button_rects.close, cursor_point).as_bool() {
                CustomTitleBarHoveredButton::Close
            } else {
                CustomTitleBarHoveredButton::None
            };

            if title_bar_hovered_button != new_hovered_button {
                // You could do tighter invalidation here but probably doesn't matter
                InvalidateRect(handle, Some(&button_rects.close), None);
                InvalidateRect(handle, Some(&button_rects.minimize), None);
                InvalidateRect(handle, Some(&button_rects.maximize), None);

                SetWindowLongPtrW(handle, GWLP_USERDATA, new_hovered_button as _);
            }

            return DefWindowProcW(handle, message, w_param, l_param);
        }
        // If the mouse gets into the client area then no title bar buttons are hovered
        // so need to reset the hover state
        WM_MOUSEMOVE => {
            if title_bar_hovered_button != CustomTitleBarHoveredButton::None {
                let result = win32_titlebar_rect(handle);
                if result.is_err() {
                    eprintln!("Failed to get title bar rect:\n{}", result.err().unwrap());
                    return DefWindowProcW(handle, message, w_param, l_param);
                }

                let title_bar_rect = result.unwrap();
                // You could do tighter invalidation here but probably doesn't matter
                InvalidateRect(handle, Some(&title_bar_rect), None);
                SetWindowLongPtrW(
                    handle,
                    GWLP_USERDATA,
                    CustomTitleBarHoveredButton::None as _,
                );
            }

            return DefWindowProcW(handle, message, w_param, l_param);
        }
        WM_NCLBUTTONDOWN => {
            // Clicks on buttons will be handled in WM_NCLBUTTONUP, but we still need
            // to remove default handling of the click to avoid it counting as drag.
            //
            // Ideally you also want to check that the mouse hasn't moved out or too much
            // between DOWN and UP messages.
            if title_bar_hovered_button != CustomTitleBarHoveredButton::None {
                return LRESULT(0);
            }

            // Default handling allows for dragging and double click to maximize
            return DefWindowProcW(handle, message, w_param, l_param);
        }
        // Map button clicks to the right messages for the window
        WM_NCLBUTTONUP => match title_bar_hovered_button {
            CustomTitleBarHoveredButton::Close => {
                if let Err(e) = PostMessageW(handle, WM_CLOSE, WPARAM(0), LPARAM(0)) {
                    eprintln!("Failed to post message: {}", e.message());
                    return DefWindowProcW(handle, message, w_param, l_param);
                }

                return LRESULT(0);
            }
            CustomTitleBarHoveredButton::Minimize => {
                ShowWindow(handle, SW_MINIMIZE);
                return LRESULT(0);
            }
            CustomTitleBarHoveredButton::Maximize => {
                let mode = if matches!(win32_window_is_maximized(handle), Ok(true)) {
                    SW_NORMAL
                } else {
                    SW_MAXIMIZE
                };

                ShowWindow(handle, mode);
                return LRESULT(0);
            }
            _ => {
                return DefWindowProcW(handle, message, w_param, l_param);
            }
        },
        WM_SETCURSOR => {
            // Show an arrow instead of the busy cursor
            let result = LoadCursorW(None, IDC_ARROW);
            if result.is_err() {
                eprintln!("Failed to load cursor: {}", result.err().unwrap().message());
                return DefWindowProcW(handle, message, w_param, l_param);
            }

            let cursor = result.unwrap();
            SetCursor(cursor);
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }

    DefWindowProcW(handle, message, w_param, l_param)
}
