//! Custom GC handlers must expose Rust-held roots alongside engine properties.
use std::{os::raw::c_int, ptr};

use ext_php_rs::{
    builders::ClassBuilder,
    class::RegisteredClass,
    ffi::zend_get_gc_buffer,
    prelude::*,
    types::{ZendClassObject, ZendHashTable, ZendObject, Zval},
};

// These exported Zend functions manage the engine's scratch buffer. They copy
// borrowed zvals without increasing or decreasing the children's refcounts.
unsafe extern "C" {
    fn zend_get_gc_buffer_create() -> *mut zend_get_gc_buffer;
    fn zend_get_gc_buffer_grow(buffer: *mut zend_get_gc_buffer);
}

#[php_class]
#[php(modifier = register_gc)]
pub struct GcRoots {
    callback: Zval,
    visible: Zval,
}

#[php_impl]
impl GcRoots {
    pub fn __construct() -> Self {
        Self {
            callback: Zval::null(),
            visible: Zval::null(),
        }
    }

    pub fn set_callback(&mut self, callback: &Zval) {
        self.callback = callback.shallow_clone();
    }

    pub fn get_visible(&self) -> Zval {
        self.visible.shallow_clone()
    }

    pub fn set_visible(&mut self, visible: &Zval) {
        self.visible = visible.shallow_clone();
    }
}

fn register_gc(builder: ClassBuilder) -> ClassBuilder {
    // SAFETY: gc_roots reports both Rust fields and Zend's ordinary properties,
    // and handles uninitialized payloads. It never calls into PHP or unwinds.
    let _ = unsafe { GcRoots::get_metadata().initialize_gc_handler(gc_roots) };
    builder
}

unsafe fn add_root(buffer: *mut zend_get_gc_buffer, root: *const Zval) {
    unsafe {
        if (*buffer).cur == (*buffer).end {
            zend_get_gc_buffer_grow(buffer);
        }
        // Equivalent to ZVAL_COPY_VALUE: the collector borrows these values.
        ptr::copy_nonoverlapping(root, (*buffer).cur, 1);
        (*buffer).cur = (*buffer).cur.add(1);
    }
}

unsafe extern "C" fn gc_roots(
    object: *mut ZendObject,
    table: *mut *mut Zval,
    count: *mut c_int,
) -> *mut ZendHashTable {
    unsafe {
        let buffer = zend_get_gc_buffer_create();
        if let Some(roots) = ZendClassObject::<GcRoots>::from_zend_obj(&*object) {
            add_root(buffer, &raw const roots.callback);
            add_root(buffer, &raw const roots.visible);
        }
        // Preserve both forms of engine storage. Do not call get_properties:
        // that handler can evaluate Rust getters and modify the object graph.
        if (*object).properties.is_null() {
            for index in 0..(*(*object).ce).default_properties_count {
                add_root(
                    buffer,
                    (*object).properties_table.as_ptr().offset(index as isize),
                );
            }
        }
        *table = (*buffer).start;
        // Never unwind through Zend, even if a fixture exceeds its count type.
        *count = c_int::try_from((*buffer).cur.offset_from((*buffer).start))
            .unwrap_or_else(|_| std::process::abort());
        (*object).properties
    }
}

// A different valid callback lets the test prove a rejected second initializer
// does not overwrite the installed callback.
unsafe extern "C" fn other_gc(
    object: *mut ZendObject,
    table: *mut *mut Zval,
    count: *mut c_int,
) -> *mut ZendHashTable {
    unsafe { gc_roots(object, table, count) }
}

#[php_function]
pub fn gc_handler_is_immutable() -> bool {
    let metadata = GcRoots::get_metadata();
    let address = ptr::from_ref(metadata.handlers());
    // SAFETY: other_gc delegates to the valid callback. Initialization must be
    // rejected because register_gc has already installed the handler.
    let rejected = !unsafe { metadata.initialize_gc_handler(other_gc) };
    rejected
        && ptr::eq(address, ptr::from_ref(metadata.handlers()))
        && metadata.handlers().get_gc.is_some_and(|callback| {
            ptr::fn_addr_eq(callback, gc_roots as unsafe extern "C" fn(_, _, _) -> _)
        })
}

pub fn build_module(builder: ModuleBuilder) -> ModuleBuilder {
    builder
        .class::<GcRoots>()
        .function(wrap_function!(gc_handler_is_immutable))
}

#[cfg(test)]
mod tests {
    #[test]
    fn gc() {
        crate::integration::test::run_php("gc/gc.php");
    }
}
