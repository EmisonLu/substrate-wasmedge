use sp_wasm_interface::Value;
use wasmedge_sdk::Memory;

/// Converts a [`wasmedge_sdk::types::Val`] into a substrate runtime interface [`Value`].
///
/// Panics if the given value doesn't have a corresponding variant in `Value`.
pub fn from_wasmedge_val(val: wasmedge_sdk::types::Val) -> Value {
	match val {
		wasmedge_sdk::types::Val::I32(v) => Value::I32(v),
		wasmedge_sdk::types::Val::I64(v) => Value::I64(v),
		wasmedge_sdk::types::Val::F32(v) => Value::F32(v as u32),
		wasmedge_sdk::types::Val::F64(v) => Value::F64(v as u64),
		v => panic!("Given value type is unsupported by Substrate: {:?}", v),
	}
}

/// Converts a sp_wasm_interface's [`Value`] into the corresponding variant in wasmedge's
/// [`wasmedge_sdk::types::Val`].
pub fn into_wasmedge_val(value: Value) -> wasmedge_sdk::types::Val {
	match value {
		Value::I32(v) => wasmedge_sdk::types::Val::I32(v),
		Value::I64(v) => wasmedge_sdk::types::Val::I64(v),
		Value::F32(f_bits) => wasmedge_sdk::types::Val::F32(f_bits as f32),
		Value::F64(f_bits) => wasmedge_sdk::types::Val::F64(f_bits as f64),
	}
}

/// Converts a [`wasmedge_sys::WasmValue`] into a substrate runtime interface [`Value`].
///
/// Panics if the given value doesn't have a corresponding variant in `Value`.
pub fn from_wasmedge_val_1(val: wasmedge_sys::WasmValue) -> Value {
	match val.ty() {
		wasmedge_types::ValType::I32 => Value::I32(val.to_i32()),
		wasmedge_types::ValType::I64 => Value::I64(val.to_i64()),
		wasmedge_types::ValType::F32 => Value::F32(val.to_f32() as u32),
		wasmedge_types::ValType::F64 => Value::F64(val.to_f64() as u64),
		v => panic!("Given value type is unsupported by Substrate: {:?}", v),
	}
}

/// Converts a sp_wasm_interface's [`Value`] into the corresponding variant in wasmedge's
/// [`wasmedge_sys::WasmValue`].
pub fn into_wasmedge_val_1(value: Value) -> wasmedge_sys::WasmValue {
	match value {
		Value::I32(v) => wasmedge_sys::WasmValue::from_i32(v),
		Value::I64(v) => wasmedge_sys::WasmValue::from_i64(v),
		Value::F32(f_bits) => wasmedge_sys::WasmValue::from_f32(f_bits as f32),
		Value::F64(f_bits) => wasmedge_sys::WasmValue::from_f64(f_bits as f64),
	}
}

pub(crate) fn memory_slice_mut(memory: &mut Memory) -> &mut [u8] {
	let base_ptr_mut: *mut u8 = memory
		.data_pointer_mut(0, 1)
		.expect("failed to returns the mut data pointer to the Memory.");

	unsafe { std::slice::from_raw_parts_mut(base_ptr_mut, (memory.size() * 64 * 1024) as usize) }
}
