use gtk::{gio, glib, prelude::*};
use glib::translate::{FromGlibPtrFull, ToGlibPtr};
use std::{ffi::CString, ptr::NonNull};
use webkit2gtk::UserContentManager;
use webkit2gtk_sys as ffi;

glib::wrapper! {
    pub struct UserContentFilter(Object<ffi::WebKitUserContentFilter>);

    match fn {
        type_ => || ffi::webkit_user_content_filter_get_type(),
    }
}

glib::wrapper! {
    pub struct UserContentFilterStore(Object<ffi::WebKitUserContentFilterStore>);

    match fn {
        type_ => || ffi::webkit_user_content_filter_store_get_type(),
    }
}

impl UserContentFilterStore {
    pub fn new(path: &str) -> Self {
        let path = CString::new(path).expect("filter store path cannot contain NUL");
        unsafe { glib::translate::from_glib_full(ffi::webkit_user_content_filter_store_new(path.as_ptr())) }
    }

    pub fn save<P: FnOnce(Result<UserContentFilter, glib::Error>) + 'static>(
        &self,
        identifier: &str,
        source: &glib::Bytes,
        cancellable: Option<&gio::Cancellable>,
        callback: P,
    ) {
        let user_data = Box::new(glib::thread_guard::ThreadGuard::new(callback));

        unsafe extern "C" fn trampoline<P: FnOnce(Result<UserContentFilter, glib::Error>) + 'static>(
            source_object: *mut glib::gobject_ffi::GObject,
            result: *mut gio::ffi::GAsyncResult,
            user_data: glib::ffi::gpointer,
        ) {
            let mut error = std::ptr::null_mut();
            let filter = ffi::webkit_user_content_filter_store_save_finish(
                source_object as *mut ffi::WebKitUserContentFilterStore,
                result,
                &mut error,
            );
            let result = if error.is_null() {
                NonNull::new(filter)
                    .map(|ptr| UserContentFilter::from_glib_full(ptr.as_ptr()))
                    .ok_or_else(|| glib::Error::new(glib::FileError::Failed, "WebKitGTK returned no filter"))
            } else {
                Err(glib::translate::from_glib_full(error))
            };
            let callback: Box<glib::thread_guard::ThreadGuard<P>> = Box::from_raw(user_data as *mut _);
            callback.into_inner()(result);
        }

        let identifier = CString::new(identifier).expect("filter identifier cannot contain NUL");
        unsafe {
            ffi::webkit_user_content_filter_store_save(
                self.to_glib_none().0,
                identifier.as_ptr(),
                source.to_glib_none().0,
                cancellable.to_glib_none().0,
                Some(trampoline::<P>),
                Box::into_raw(user_data) as _,
            );
        }
    }
}

pub fn add_filter(manager: &UserContentManager, filter: &UserContentFilter) {
    unsafe {
        ffi::webkit_user_content_manager_add_filter(
            manager.to_glib_none().0,
            filter.to_glib_none().0,
        );
    }
}
