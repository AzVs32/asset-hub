;; The single export takes protobuf Request and returns protobuf Reply. All
;; top-level Request fields are length-delimited. Tests inject prepare failures.
(component
(core module $guest
  (memory (export "memory") 1)
  (data (i32.const 1024) "@@DESCRIPTION@@")
  (data (i32.const 2048) "@@UNIT@@")
  (data (i32.const 3072) "@@STORAGE@@")
  (data (i32.const 3328) "@@ENTRY@@")
  (data (i32.const 3456) "@@READER@@")
  (data (i32.const 3584) "@@WRITER@@")
  (data (i32.const 3712) "@@CHUNK@@")
  (data (i32.const 3840) "@@EOF@@")
  (data (i32.const 3968) "@@LOOKUP@@")
  (data (i32.const 4096) "@@OBSERVED@@")
  (data (i32.const 4224) "@@MISSING@@")
  (global $heap (mut i32) (i32.const 4608))
  (global $writing (mut i32) (i32.const 0))
  (global $initialized (mut i32) (i32.const 0))
  (global $next (mut i32) (i32.const 1))
  (global $read (mut i32) (i32.const 0))
  (func (export "cabi_realloc") (param i32 i32 i32) (param $size i32) (result i32)
    (local $pointer i32)
    global.get $heap local.tee $pointer local.get $size i32.add
    i32.const 7 i32.add i32.const -8 i32.and global.set $heap
    local.get $pointer)
  (func $varint (param $pos i32) (result i32 i32)
    (local $value i32) (local $shift i32) (local $byte i32)
    (loop $read
      local.get $pos i32.load8_u local.set $byte
      local.get $value local.get $byte i32.const 127 i32.and
      local.get $shift i32.shl i32.or local.set $value
      local.get $pos i32.const 1 i32.add local.set $pos
      local.get $shift i32.const 7 i32.add local.set $shift
      local.get $byte i32.const 128 i32.and br_if $read)
    local.get $value local.get $pos)
  (func (export "assethub:vfs/driver@4.0.0#call") (param $ptr i32) (param $length i32) (result i32)
    (local $pos i32) (local $tag i32) (local $size i32) (local $op i32)
    i32.const 0 i32.const 1024 i32.store
    i32.const 4 i32.const @@DESCRIPTION_LENGTH@@ i32.store
    i32.const 16 i32.const 2048 i32.store
    i32.const 20 i32.const @@UNIT_LENGTH@@ i32.store
    i32.const 32 i32.const 3072 i32.store
    i32.const 36 i32.const @@STORAGE_LENGTH@@ i32.store
    i32.const 48 i32.const 3328 i32.store
    i32.const 52 i32.const @@ENTRY_LENGTH@@ i32.store
    i32.const 64 i32.const 3456 i32.store
    i32.const 68 i32.const @@READER_LENGTH@@ i32.store
    i32.const 80 i32.const 3584 i32.store
    i32.const 84 i32.const @@WRITER_LENGTH@@ i32.store
    i32.const 96 i32.const 3712 i32.store
    i32.const 100 i32.const @@CHUNK_LENGTH@@ i32.store
    i32.const 112 i32.const 3840 i32.store
    i32.const 116 i32.const @@EOF_LENGTH@@ i32.store
    i32.const 128 i32.const 3968 i32.store
    i32.const 132 i32.const @@LOOKUP_LENGTH@@ i32.store
    i32.const 144 i32.const 4096 i32.store
    i32.const 148 i32.const @@OBSERVED_LENGTH@@ i32.store
    i32.const 160 i32.const 4224 i32.store
    i32.const 164 i32.const @@MISSING_LENGTH@@ i32.store
    local.get $ptr local.set $pos
    (block $found
      (loop $field
        local.get $pos local.get $ptr local.get $length i32.add i32.ge_u br_if $found
        local.get $pos call $varint local.set $pos local.set $tag
        local.get $pos call $varint local.set $pos local.set $size
        local.get $tag i32.const 3 i32.shr_u local.tee $op i32.const 3 i32.ge_u br_if $found
        local.get $pos local.get $size i32.add local.set $pos
        br $field))
    local.get $op i32.const 3 i32.eq if i32.const 0 return end
    local.get $op i32.const 4 i32.eq if @@INITIALIZE@@ return end
    local.get $op i32.const 5 i32.eq if @@PREPARE@@ return end
    local.get $op i32.const 8 i32.eq if
      global.get $initialized i32.const 1 i32.ne if unreachable end
      local.get $pos i32.const 2 i32.add i32.load8_u i32.const 98 i32.eq if unreachable end
      i32.const 32 return end
    local.get $op i32.const 17 i32.eq if
      ;; handle is 1. The first byte of the purpose follows its tag and length.
      local.get $pos i32.const 4 i32.add i32.load8_u i32.const 119 i32.eq global.set $writing
      global.get $next i32.const 1 i32.add global.set $next
      i32.const 3972 global.get $next i32.store8
      i32.const 128 return end
    local.get $op i32.const 18 i32.eq if
      global.get $writing if i32.const 160 return end
      ;; Skip the lookup, parent, and the name tag and length.
      i32.const 6 local.set $size
      (loop $observe_field
        local.get $pos call $varint local.set $pos drop
        local.get $size i32.const 1 i32.sub local.tee $size br_if $observe_field)
      @@STAT@@ return end
    local.get $op i32.const 21 i32.eq if
      local.get $pos call $varint local.set $pos drop
      local.get $pos call $varint local.set $pos drop
      local.get $pos call $varint local.set $pos i32.const 3 i32.shr_u local.set $tag
      local.get $tag i32.const 3 i32.eq if
        global.get $next i32.const 1 i32.add global.set $next
        i32.const @@READER_HANDLE@@ global.get $next i32.store8
        i32.const 0 global.set $read i32.const 64 return end
      local.get $tag i32.const 4 i32.eq if
        global.get $next i32.const 1 i32.add global.set $next
        i32.const @@WRITER_HANDLE@@ global.get $next i32.store8
        i32.const 80 return end
      i32.const 48 return end
    local.get $op i32.const 12 i32.eq if
      global.get $read if i32.const 112 return end
      i32.const 1 global.set $read i32.const 96 return end
    local.get $op i32.const 14 i32.eq if @@COMMIT@@ return end
    i32.const 16)
)
(core instance $guest (instantiate $guest))
(alias core export $guest "memory" (core memory $memory))
(alias core export $guest "cabi_realloc" (core func $realloc))
(alias core export $guest "assethub:vfs/driver@4.0.0#call" (core func $call))
(func $call (param "request" (list u8)) (result (list u8))
  (canon lift (core func $call) (memory $memory) (realloc $realloc)))
(instance $driver (export "call" (func $call)))
(export "assethub:vfs/driver@4.0.0" (instance $driver))
)
