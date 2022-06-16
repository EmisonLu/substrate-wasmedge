use crate::host::HostState;
use sc_executor_common::{
	error::{Backtrace, Error, MessageWithBacktrace, Result, WasmError},
	wasm_runtime::InvokeMethod,
};
use sp_wasm_interface::{Pointer, Value, WordSize};
use std::sync::{Arc, Mutex};
use wasmedge_sys::Vm;

pub struct InstanceWrapper {
	vm: Arc<Mutex<Vm>>,
	instance: Option<wasmedge_sys::Instance>,
	memory: Option<wasmedge_sys::Memory>,
	host_state: Option<HostState>,
}

impl InstanceWrapper {
	pub fn new(vm: Arc<Mutex<Vm>>) -> Arc<Mutex<Self>> {
		Arc::new(Mutex::new(InstanceWrapper { vm, instance: None, memory: None, host_state: None }))
	}

	pub fn instantiate(&mut self) -> Result<()> {
		self.vm
			.lock()
			.map_err(|e| WasmError::Other(format!("failed to lock: {}", e,)))?
			.instantiate()
			.map_err(|e| {
				WasmError::Other(
					format!("failed to instantiate a new WASM module instance: {}", e,),
				)
			})?;

		let instance = self
			.vm
			.lock()
			.map_err(|e| WasmError::Other(format!("failed to lock: {}", e,)))?
			.active_module()
			.map_err(|e| WasmError::Other(format!("failed to get WASM instance: {}", e,)))?;

		let memory = instance.get_memory("memory").map_err(|e| {
			WasmError::Other(format!("failed to get WASM memory named 'memory': {}", e,))
		})?;

		self.instance = Some(instance);
		self.memory = Some(memory);
		Ok(())
	}

	pub fn call(
		&mut self,
		method: InvokeMethod,
		data_ptr: Pointer<u8>,
		data_len: WordSize,
	) -> Result<u64> {
		let data_ptr = wasmedge_sys::WasmValue::from_f32(u32::from(data_ptr) as f32);
		let data_len = wasmedge_sys::WasmValue::from_f32(u32::from(data_len) as f32);
		let res: std::result::Result<
			Vec<wasmedge_sys::WasmValue>,
			wasmedge_types::error::WasmEdgeError,
		>;

		let mut executor = wasmedge_sys::Executor::create(None, None).map_err(|e| {
			WasmError::Other(format!("fail to create a WasmEdge Executor context: {}", e))
		})?;

		match method {
			InvokeMethod::Export(method) => {
				let func = self.instance().get_func(method).map_err(|error| {
					WasmError::Other(format!("function is not found: {}", error,))
				})?;

				let func_type = func.ty().map_err(|error| {
					WasmError::Other(format!("fail to get the function type: {}", error,))
				})?;

				if func_type.params_len() != 2 || func_type.returns_len() != 1 {
					return Err(Error::Other(format!("Invalid signature for direct entry point")));
				}

				res = func.call(&mut executor, vec![data_ptr, data_len]);
			},
			InvokeMethod::Table(func_ref) => {
				let table =
					self.instance().get_table("__indirect_function_table").map_err(|error| {
						WasmError::Other(format!(
							"table named '__indirect_function_table' is not found: {}",
							error,
						))
					})?;

				let func_ref = table
					.get_data(func_ref)
					.map_err(|error| {
						WasmError::Other(format!("failed to get the data: {}", error,))
					})?
					.func_ref();

				if let Some(func_ref) = func_ref {
					res = func_ref.call(&mut executor, vec![data_ptr, data_len]);
				} else {
					return Err(sc_executor_common::error::Error::Other(format!(
						"the WasmValue is a NullRef"
					)));
				}
			},
			InvokeMethod::TableWithWrapper { dispatcher_ref, func } => {
				let table =
					self.instance().get_table("__indirect_function_table").map_err(|error| {
						WasmError::Other(format!(
							"table named '__indirect_function_table' is not found: {}",
							error,
						))
					})?;
				let func_ref = table
					.get_data(dispatcher_ref)
					.map_err(|error| {
						WasmError::Other(format!("failed to get the data: {}", error,))
					})?
					.func_ref();

				if let Some(func_ref) = func_ref {
					res = func_ref.call(
						&mut executor,
						vec![wasmedge_sys::WasmValue::from_f32(func as f32), data_ptr, data_len],
					);
				} else {
					return Err(sc_executor_common::error::Error::Other(format!(
						"the WasmValue is a NullRef"
					)));
				}
			},
		};

		let s = res.map_err(|trap| {
			let mut backtrace_string = trap.to_string();
			let suffix = "\nwasm backtrace:";
			if let Some(index) = backtrace_string.find(suffix) {
				// Get rid of the error message and just grab the backtrace,
				// since we're storing the error message ourselves separately.
				backtrace_string.replace_range(0..index + suffix.len(), "");
			}

			let backtrace = Backtrace { backtrace_string };
			if let Some(error) = self
				.host_state_mut()
				.expect("host state cannot be empty while a function is being called; qed")
				.take_panic_message()
			{
				Error::AbortedDueToPanic(MessageWithBacktrace {
					message: error,
					backtrace: Some(backtrace),
				})
			} else {
				Error::AbortedDueToTrap(MessageWithBacktrace {
					message: trap.to_string(),
					backtrace: Some(backtrace),
				})
			}
		})?;

		Ok(s[0].to_f64() as u64)
	}

	pub fn extract_heap_base(&mut self) -> Result<u32> {
		let heap_base_export = self.instance().get_global("__heap_base").map_err(|error| {
			WasmError::Other(format!("failed to get WASM global named '__heap_base': {}", error,))
		})?;

		let heap_base = heap_base_export.get_value();
		Ok(heap_base.to_f32() as u32)
	}

	pub fn get_global_val(&mut self, name: &str) -> Result<Option<Value>> {
		let global = self
			.instance()
			.get_global(name)
			.map_err(|error| Error::Other(format!("failed to get WASM global: {}", error,)))?
			.get_value();

		match global.ty() {
			wasmedge_types::ValType::I32 => Ok(Some(Value::I32(global.to_i32()))),
			wasmedge_types::ValType::I64 => Ok(Some(Value::I64(global.to_i64()))),
			wasmedge_types::ValType::F32 => Ok(Some(Value::F32(global.to_f32() as u32))),
			wasmedge_types::ValType::F64 => Ok(Some(Value::F64(global.to_f64() as u64))),
			_ => Err("Unknown value type".into()),
		}
	}

	pub fn get_global(&mut self, name: &str) -> Result<wasmedge_sys::Global> {
		self.instance()
			.get_global(name)
			.map_err(|error| Error::Other(format!("failed to get WASM global: {}", error,)))
	}

	pub fn base_ptr(&self) -> *const u8 {
		self.memory()
			.data_pointer(0, 1)
			.expect("failed to returns the const data pointer to the Memory.")
	}

	pub fn base_ptr_mut(&mut self) -> *mut u8 {
		self.memory_mut()
			.data_pointer_mut(0, 1)
			.expect("failed to returns the mut data pointer to the Memory.")
	}

	pub(crate) fn memory(&self) -> &wasmedge_sys::Memory {
		&self.memory.as_ref().unwrap()
	}

	pub(crate) fn memory_mut(&mut self) -> &mut wasmedge_sys::Memory {
		self.memory.as_mut().unwrap()
	}

	pub(crate) fn instance(&self) -> &wasmedge_sys::Instance {
		&self.instance.as_ref().unwrap()
	}

	pub(crate) fn vm(&self) -> Arc<Mutex<Vm>> {
		Arc::clone(&self.vm)
	}

	pub fn memory_slice_mut(&mut self) -> &mut [u8] {
		unsafe {
			std::slice::from_raw_parts_mut(
				self.base_ptr_mut(),
				(self.memory().size() * 64 * 1024 * 8) as usize,
			)
		}
	}

	pub fn memory_slice(&self) -> &[u8] {
		unsafe {
			std::slice::from_raw_parts(
				self.base_ptr(),
				(self.memory().size() * 64 * 1024 * 8) as usize,
			)
		}
	}

	pub fn host_state(&self) -> Option<&HostState> {
		self.host_state.as_ref()
	}

	pub fn host_state_mut(&mut self) -> Option<&mut HostState> {
		self.host_state.as_mut()
	}

	pub fn set_host_state(&mut self, host_state: Option<HostState>) {
		self.host_state = host_state;
	}

	pub fn allocate_memory(
		&mut self,
		size: sp_wasm_interface::WordSize,
	) -> sp_wasm_interface::Result<Pointer<u8>> {
		let memory_slice = unsafe {
			std::slice::from_raw_parts_mut(
				self.base_ptr_mut(),
				(self.memory().size() * 64 * 1024 * 8) as usize,
			)
		};

		self.host_state_mut()
			.expect("host state is not empty when calling a function in wasm; qed")
			.allocator()
			.allocate(memory_slice, size)
			.map_err(|e| e.to_string())
	}

	pub fn deallocate_memory(
		&mut self,
		ptr: sp_wasm_interface::Pointer<u8>,
	) -> sp_wasm_interface::Result<()> {
		let memory_slice = unsafe {
			std::slice::from_raw_parts_mut(
				self.base_ptr_mut(),
				(self.memory().size() * 64 * 1024 * 8) as usize,
			)
		};

		self.host_state_mut()
			.expect("host state is not empty when calling a function in wasm; qed")
			.allocator()
			.deallocate(memory_slice, ptr)
			.map_err(|e| e.to_string())
	}

	pub fn decommit(&mut self) {
		if self.memory().size() == 0 {
			return;
		}

		cfg_if::cfg_if! {
			if #[cfg(target_os = "linux")] {
				use std::sync::Once;

				unsafe {
					let ptr = self.base_ptr();
					let len = (self.memory().size() * 64 * 1024) as usize;

					if libc::madvise(ptr as _, len, libc::MADV_DONTNEED) != 0 {
						static LOGGED: Once = Once::new();
						LOGGED.call_once(|| {
							log::warn!(
								"madvise(MADV_DONTNEED) failed: {}",
								std::io::Error::last_os_error(),
							);
						});
					} else {
						return;
					}
				}
			} else if #[cfg(target_os = "macos")] {
				use std::sync::Once;

				unsafe {
					let ptr = self.base_ptr();
					let len = (self.memory().size() * 64 * 1024) as usize;

					if libc::mmap(
						ptr as _,
						len,
						libc::PROT_READ | libc::PROT_WRITE,
						libc::MAP_FIXED | libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
						-1,
						0,
					) == libc::MAP_FAILED {
						static LOGGED: Once = Once::new();
						LOGGED.call_once(|| {
							log::warn!(
								"Failed to decommit WASM instance memory through mmap: {}",
								std::io::Error::last_os_error(),
							);
						});
					} else {
						return;
					}
				}
			}
		}

		let memory_slice: &mut [u8];
		unsafe {
			memory_slice = std::slice::from_raw_parts_mut(
				self.base_ptr_mut(),
				(self.memory().size() * 64 * 1024 * 8) as usize,
			);
		}
		memory_slice.fill(0);
	}
}
