;; Minimal sync canonical ABI fixture. Only describe, initialize and connection
;; probes succeed. prepare injects a trap or an infinite loop from the test.
(module
  (import "[export]assethub:vfs/driver@1.0.0" "[resource-drop]reader" (func (param i32)))
  (import "[export]assethub:vfs/driver@1.0.0" "[resource-new]reader" (func (param i32) (result i32)))
  (import "[export]assethub:vfs/driver@1.0.0" "[resource-rep]reader" (func (param i32) (result i32)))
  (import "[export]assethub:vfs/driver@1.0.0" "[resource-drop]writer" (func (param i32)))
  (import "[export]assethub:vfs/driver@1.0.0" "[resource-new]writer" (func (param i32) (result i32)))
  (import "[export]assethub:vfs/driver@1.0.0" "[resource-rep]writer" (func (param i32) (result i32)))
  (import "[export]assethub:vfs/driver@1.0.0" "[resource-drop]storage" (func (param i32)))
  (import "[export]assethub:vfs/driver@1.0.0" "[resource-new]storage" (func (param i32) (result i32)))
  (import "[export]assethub:vfs/driver@1.0.0" "[resource-rep]storage" (func (param i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "@@DESCRIPTION@@")
  (global $heap (mut i32) (i32.const 4096))
  (global $initialized (mut i32) (i32.const 0))
  (func (export "cabi_realloc") (param i32 i32 i32) (param $size i32) (result i32)
    (local $pointer i32)
    global.get $heap
    local.tee $pointer
    local.get $size
    i32.add
    i32.const 7
    i32.add
    i32.const -8
    i32.and
    global.set $heap
    local.get $pointer)
  (func (export "assethub:vfs/driver@1.0.0#describe") (result i32)
    i32.const 0 i32.const 1024 i32.store
    i32.const 4 i32.const @@DESCRIPTION_LENGTH@@ i32.store
    i32.const 0)
  (func (export "assethub:vfs/driver@1.0.0#initialize") (param i32 i32) (result i32)
    @@INITIALIZE@@)
  (func (export "assethub:vfs/driver@1.0.0#test-connection") (param i32 i32) (result i32)
    i32.const 16)
  (func (export "assethub:vfs/driver@1.0.0#prepare") (param i32 i32) (result i32)
    @@PREPARE@@)
  (func (export "assethub:vfs/driver@1.0.0#migrate") (param i32 i32 i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#instances") (param i32 i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#open") (param i32 i32 i32 i32 i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#[method]storage.info") (param i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#[method]storage.call") (param i32 i32 i32 i32 i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#[method]storage.read") (param i32 i32 i32 i64 i32 i64) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#[method]storage.write") (param i32 i32 i32 i32 i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#[method]reader.next") (param i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#[method]writer.write") (param i32 i32 i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#[method]writer.commit") (param i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#[method]writer.abort") (param i32) (result i32) unreachable)
  (func (export "assethub:vfs/driver@1.0.0#[dtor]reader") (param i32))
  (func (export "assethub:vfs/driver@1.0.0#[dtor]writer") (param i32))
  (func (export "assethub:vfs/driver@1.0.0#[dtor]storage") (param i32))
)
