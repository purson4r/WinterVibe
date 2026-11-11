use std::mem::size_of;

use windows::Win32::Foundation::{HWND, HINSTANCE};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TDF_ALLOW_DIALOG_CANCELLATION, TDF_POSITION_RELATIVE_TO_WINDOW, TD_ERROR_ICON,
    TD_INFORMATION_ICON, TD_WARNING_ICON, TaskDialogIndirect,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use crate::log;

#[derive(Clone, Copy)]
pub enum Icon {
    Information,
    Warning,
    Error,
}

pub struct Button<'a> {
    pub id: i32,
    pub label: &'a str,
}

pub fn show(
    parent: Option<HWND>,
    title: &str,
    instruction: &str,
    content: &str,
    buttons: &[Button<'_>],
    default_button: i32,
    icon: Icon,
) -> i32 {
    let wide_title = wide(title);
    let wide_instruction = wide(instruction);
    let wide_content = wide(content);
    let wide_buttons: Vec<Vec<u16>> = buttons.iter().map(|button| wide(button.label)).collect();
    let native_buttons: Vec<TASKDIALOG_BUTTON> = buttons
        .iter()
        .zip(&wide_buttons)
        .map(|(button, label)| TASKDIALOG_BUTTON { nButtonID: button.id, pszButtonText: PCWSTR(label.as_ptr()) })
        .collect();
    let icon = match icon {
        Icon::Information => TD_INFORMATION_ICON,
        Icon::Warning => TD_WARNING_ICON,
        Icon::Error => TD_ERROR_ICON,
    };
    let config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: parent.unwrap_or_default(),
        hInstance: HINSTANCE::default(),
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
        pszWindowTitle: PCWSTR(wide_title.as_ptr()),
        Anonymous1: TASKDIALOGCONFIG_0 { pszMainIcon: icon },
        pszMainInstruction: PCWSTR(wide_instruction.as_ptr()),
        pszContent: PCWSTR(wide_content.as_ptr()),
        cButtons: native_buttons.len() as u32,
        pButtons: native_buttons.as_ptr(),
        nDefaultButton: default_button,
        ..Default::default()
    };
    let mut selected = 0;
    match unsafe { TaskDialogIndirect(&config, Some(&mut selected), None, None) } {
        Ok(()) => selected,
        Err(error) => {
            log!("native task dialog failed: {error}");
            fallback(parent, &wide_title, &wide_instruction, &wide_content, buttons)
        }
    }
}

fn fallback(parent: Option<HWND>, title: &[u16], instruction: &[u16], content: &[u16], buttons: &[Button<'_>]) -> i32 {
    let mut message = String::from_utf16_lossy(instruction.strip_suffix(&[0]).unwrap_or(instruction));
    if content.len() > 1 {
        message.push_str("\n\n");
        message.push_str(&String::from_utf16_lossy(content.strip_suffix(&[0]).unwrap_or(content)));
    }
    let message = wide(&message);
    let yes_no = buttons.len() == 2;
    let result = unsafe {
        MessageBoxW(
            parent,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_SETFOREGROUND | MB_TOPMOST | if yes_no { MB_YESNO | MB_ICONINFORMATION } else { MB_OK | MB_ICONINFORMATION },
        )
    };
    if yes_no && result == IDYES {
        buttons[0].id
    } else if yes_no {
        buttons[1].id
    } else {
        buttons.first().map_or(0, |button| button.id)
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
