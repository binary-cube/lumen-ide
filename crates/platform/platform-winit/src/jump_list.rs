//! The taskbar's jump list (Windows): the recent projects, as a category of their own;
//! each starts the app with the project's path (the running instance takes it). The open
//! windows are already the taskbar's thumbnails.

use platform_api::IconMenu;
use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
};
use windows::core::{HSTRING, Interface};

/// Replaces the jump list's projects.
pub(crate) fn set(menu: &IconMenu) {
    if let Err(e) = build(menu) {
        tracing::warn!(error = %e, "can't update the jump list");
    }
}

fn build(menu: &IconMenu) -> windows::core::Result<()> {
    let exe = std::env::current_exe().map_err(|_| windows::core::Error::from_thread())?;
    let exe = HSTRING::from(exe.as_os_str());
    // SAFETY: COM on this (the UI) thread, initialized here if it isn't yet (already
    // initialized, in this or another mode: the call just says so); the interfaces are
    // the system's, used as documented.
    #[allow(unsafe_code)]
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let list: ICustomDestinationList =
            CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
        let mut slots = 0u32;
        let _removed: IObjectArray = list.BeginList(&mut slots)?;
        let items: IObjectCollection =
            CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
        for project in &menu.projects {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(&exe)?;
            link.SetArguments(&HSTRING::from(format!("\"{}\"", project.path.display())))?;
            link.SetDescription(&HSTRING::from(project.path.as_os_str()))?;
            link.SetIconLocation(&exe, 0)?;
            let store: IPropertyStore = link.cast()?;
            store.SetValue(&PKEY_Title, &PROPVARIANT::from(project.label.as_str()))?;
            store.Commit()?;
            items.AddObject(&link)?;
        }
        let array: IObjectArray = items.cast()?;
        if !menu.projects.is_empty() {
            list.AppendCategory(&HSTRING::from(menu.title.as_str()), &array)?;
        }
        list.CommitList()
    }
}
