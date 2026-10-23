<?php

assert(gc_handler_is_immutable());

// A hidden Rust field owns the closure, which owns the Rust-backed PHP object.
for ($i = 0; $i < 16; ++$i) {
    $object = new GcRoots();
    $callback = static fn() => $object;
    $object->setCallback($callback);
    $weakObject = WeakReference::create($object);
    $weakCallback = WeakReference::create($callback);
    unset($object, $callback);
    assert(gc_collect_cycles() >= 1);
    assert($weakObject->get() === null);
    assert($weakCallback->get() === null);
}

// A live external reference must keep the cycle alive. Two Rust fields can own
// separate references to the same closure; both need to be reported.
$object = new GcRoots();
$callback = static fn() => $object;
$object->setCallback($callback);
$object->setVisible($callback);
assert($object->getVisible() === $callback);
$weakObject = WeakReference::create($object);
unset($object);
gc_collect_cycles();
assert($weakObject->get() !== null);
assert($callback() === $weakObject->get());
unset($callback);
gc_collect_cycles();
assert($weakObject->get() === null);

#[AllowDynamicProperties]
class GcChild extends GcRoots
{
    public mixed $edge = null;
}

// Cover both the inline property slots and the materialized property table.
foreach ([false, true] as $materialize) {
    $object = new GcChild();
    $child = new stdClass();
    $child->parent = $object;
    $object->edge = $child;
    $object->setCallback(static fn() => $object);
    if ($materialize) {
        $object->dynamic = $child;
        $properties = get_object_vars($object);
        unset($properties);
    }
    $weakObject = WeakReference::create($object);
    $weakChild = WeakReference::create($child);
    unset($object, $child);
    gc_collect_cycles();
    assert($weakObject->get() === null);
    assert($weakChild->get() === null);
}

class UninitializedGcChild extends GcRoots
{
    public mixed $edge = null;

    public function __construct()
    {
    }
}

$object = new UninitializedGcChild();
$object->edge = $object;
$weakObject = WeakReference::create($object);
unset($object);
gc_collect_cycles();
assert($weakObject->get() === null);
