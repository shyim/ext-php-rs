# Rust-held PHP references and cycle collection

Reference counting alone cannot reclaim a cycle such as a Rust-backed PHP object
holding a PHP closure that captures that object. Zend must also be able to see
the references owned by the Rust payload when it traverses the object graph.

The default object handler reports the engine's ordinary properties. It cannot
discover arbitrary `Zval` fields, arrays, or callback registries inside a Rust
struct. Extensions that hold these references can install a custom `get_gc`
callback through `ClassMetadata::initialize_gc_handler`.

## Registering a handler

Register the callback during module initialization, before anything calls the
class metadata's `handlers()` method or creates an instance. A class builder
modifier is one place to do this:

```rust,ignore
use ext_php_rs::{builders::ClassBuilder, class::RegisteredClass, prelude::*, types::Zval};

#[php_class]
#[php(modifier = configure_gc)]
pub struct CallbackOwner {
    callback: Zval,
}

fn configure_gc(builder: ClassBuilder) -> ClassBuilder {
    // SAFETY: callback_owner_gc implements the get_gc contract for this class,
    // its PHP subclasses, and uninitialized instances.
    let installed = unsafe {
        CallbackOwner::get_metadata().initialize_gc_handler(callback_owner_gc)
    };
    // false means the immutable handler table already exists. This is expected
    // on a module restart, but on first startup can indicate registration is late.
    let _ = installed;
    builder
}
```

The method initializes the usual ext-php-rs handlers and replaces only `get_gc`.
It never changes an existing table. The handler table's identity is preserved,
so `ZendClassObject::<T>::from_zend_obj` continues to recognize the objects.
Handlers live for the process lifetime and are reused after a module restart.

## Callback obligations

This is an unsafe, low-level API. A callback receives a `ZendObject` pointer,
an output pointer to a contiguous table of borrowed zvals, and its output count.
It returns an optional property hash table. Both collections are traversed by
the collector.

- Report each owned reference exactly once. Two separately owned references to
  the same PHP value must both be reported; do not deduplicate by value.
- Include Zend's ordinary properties as well as the Rust payload. Preserve both
  inline declared-property slots and materialized property hash tables.
- Do not increment or decrement reference counts when reporting roots. The
  collector borrows them; the object retains ownership.
- Keep the returned storage valid throughout traversal. Zend's GC scratch-buffer
  API can combine noncontiguous Rust fields and declared property slots. A local
  Rust `Vec` that is dropped when the callback returns is not valid storage.
- Handle uninitialized Rust payloads, including subclasses that skip the parent
  constructor. Do not assume the object can always be cast to an initialized `T`.
- Never invoke PHP, evaluate property getters, mutate the object graph, panic,
  or unwind from the callback. In particular, do not call `get_properties` to
  synthesize roots: that handler can evaluate getters and change references.

The [GC integration fixture](https://github.com/extphprs/ext-php-rs/tree/master/tests/src/integration/gc)
demonstrates combining Rust fields with engine properties using Zend's scratch
buffer. Its tests cover callback cycles, live external references, PHP subclasses,
uninitialized payloads, and attempts to replace an initialized handler table.

This hook does not automatically trace Rust fields or add support for lazy-object
initializer/proxy references. An extension that implements additional object
behavior remains responsible for reporting the corresponding owned references.
