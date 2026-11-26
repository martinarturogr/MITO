# processes.py → mito-win::processes (crate `mito-win`, consumido por `mito-engine`)

Origen: `core/app/mito/processes.py` (147 líneas), `core/tests/test_processes.py` (1 test).

## Responsabilidad
Gestión del ciclo de vida de procesos en Windows: crear procesos suspendidos y asignarlos a un Job Object
con «kill on close» antes de reanudarlos, esperar a que el árbol del Job quede vacío, abrir un proceso
existente por PID para vigilarlo, y crear un named pipe accesible solo por su propietario y SYSTEM,
identificando al cliente por PID. Todo el módulo es Win32 mediante `ctypes`. El docstring indica
«Handles are never inherited by launched commands» (`processes.py:1`).

## API pública del original
| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `class ProcessHandle(pid, assign=False)` (`:56-66`) | PID; si `assign` pide también `PROCESS_SET_QUOTA` y `PROCESS_TERMINATE` | objeto con `handle` | `OSError` (`WinError`) si `OpenProcess` falla |
| `ProcessHandle.alive() -> bool` (`:61-62`) | — | `True` si `WaitForSingleObject(h, 0) == 258` | — |
| `ProcessHandle.close()` (`:64-66`) | — | — | Idempotente (`if self.handle`) |
| `class ProcessJob()` (`:69-111`) | — | crea Job anónimo con `KILL_ON_JOB_CLOSE` | `OSError` si `CreateJobObjectW` o `SetInformationJobObject` fallan (en este último caso cierra el handle y relanza) |
| `ProcessJob.assign(handle)` (`:79-80`) | handle de proceso (`int`) | — | `OSError` si `AssignProcessToJobObject` falla |
| `ProcessJob.terminate()` (`:82-84`) | — | — | `OSError` si `TerminateJobObject` falla; no hace nada tras `close()` |
| `ProcessJob.close()` (`:86-88`) | — | — | Idempotente. **Cerrar el Job mata a todos sus procesos** (ver invariantes) |
| `ProcessJob.active_processes() -> int` (`:90-93`) | — | `ActiveProcesses` de `JobObjectBasicAccountingInformation` | `OSError` si la consulta falla |
| `ProcessJob.wait_empty(interval=.05)` (`:95-97`) | intervalo en segundos | — | Bloquea mientras `active_processes()` > 0. Sin tiempo máximo. |
| `ProcessJob.spawn(argv, creationflags=subprocess.CREATE_NO_WINDOW, **kwargs)` (`:99-111`) | lista de argumentos, flags adicionales, resto de kwargs de `subprocess.Popen` | `subprocess.Popen` ya reanudado | `OSError('Could not resume command')` si `NtResumeProcess` falla; relanza cualquier error tras matar el proceso |
| `restrict_pipe(handle)` (`:114-124`) | handle de named pipe | — | `OSError` si la conversión SDDL o `SetKernelObjectSecurity` fallan |
| `pipe_client_pid(connection) -> int` (`:127-130`) | objeto con `fileno()` (conexión de `multiprocessing`) | PID del cliente | `OSError` si `GetNamedPipeClientProcessId` falla |
| `pipe_listener(address) -> PrivateListener` (`:133-147`) | dirección de pipe (`\\.\pipe\...`) | listener de `multiprocessing.connection` con `backlog` 1 y `_new_handle` propio | `OSError` si `CreateNamedPipe` o `restrict_pipe` fallan (cierra el handle y relanza) |
| `kernel()` (`:33-47`) | — | `ctypes.WinDLL('kernel32')` con `argtypes`/`restype` fijados | — |
| `checked(value)` (`:50-53`) | valor devuelto | el mismo valor | `OSError` con `WinError(GetLastError())` si `value` es 0/NULL |

Estructuras `ctypes` (ver «Estructuras» más abajo): `IO_COUNTERS` (`:8-10`), `BASIC_LIMIT` (`:13-17`),
`EXTENDED_LIMIT` (`:20-23`), `BASIC_ACCOUNTING` (`:26-30`).

## Formatos de disco
Ninguno. El módulo no lee ni escribe ficheros. El nombre del pipe lo elige el llamador (`address`).

## Constantes Win32 y valores literales

| Constante / valor | Valor | Dónde | Significado |
|---|---|---|---|
| `OpenProcess` access `0x100000` | `SYNCHRONIZE` | `processes.py:59` | Permite `WaitForSingleObject` sobre el proceso |
| `OpenProcess` access `0x101` | `PROCESS_SET_QUOTA` (0x100) \| `PROCESS_TERMINATE` (0x1) | `:59` | Necesario para `AssignProcessToJobObject`; solo si `assign=True` |
| `WaitForSingleObject` → `258` | `WAIT_TIMEOUT` | `:62` | El proceso sigue vivo |
| `WaitForSingleObject(h, 0)` | timeout 0 | `:62` | Sondeo sin bloquear |
| `SetInformationJobObject` clase `9` | `JobObjectExtendedLimitInformation` | `:75` | Se pasa `EXTENDED_LIMIT` |
| `LimitFlags = 0x2000` | `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` | `:73` | Al cerrar el último handle del Job, mueren sus procesos |
| `QueryInformationJobObject` clase `1` | `JobObjectBasicAccountingInformation` | `:92` | Campo `ActiveProcesses` |
| `TerminateJobObject(handle, 1)` | código de salida `1` | `:84` | Mata todos los procesos del Job |
| `creationflags` de `spawn` | `0x4` = `CREATE_SUSPENDED`, combinado con `creationflags` (por defecto `subprocess.CREATE_NO_WINDOW`) | `:101` | El proceso empieza suspendido |
| `subprocess.CREATE_NO_WINDOW` | `0x08000000` | `:99` (default) | Sin ventana de consola |
| `NtResumeProcess` (ntdll) | `c_long` de retorno; `0` = éxito | `:104-108` | Reanuda el proceso (no documentada oficialmente) |
| `ConvertStringSecurityDescriptorToSecurityDescriptorW` revisión `1` | `SDDL_REVISION_1` | `:120` | — |
| SDDL `D:P(A;;GA;;;SY)(A;;GA;;;OW)` | DACL protegida; `GA` (GENERIC_ALL) para `SY` (SYSTEM) y `OW` (propietario) | `:120` | Única ACE para SYSTEM y para el propietario |
| `SetKernelObjectSecurity` `4` | `DACL_SECURITY_INFORMATION` | `:122` | Se aplica solo el DACL |
| `_winapi.PIPE_ACCESS_DUPLEX` | — | `:138` | Pipe bidireccional |
| `_winapi.FILE_FLAG_OVERLAPPED` | — | `:138` | E/S superpuesta |
| `0x40000` | `WRITE_DAC` (inferencia: valor de `winnt.h`, **Duda** confirmar) | `:138` | Permite cambiar el DACL del pipe; sin este bit `SetKernelObjectSecurity` falla |
| `_winapi.FILE_FLAG_FIRST_PIPE_INSTANCE` | — | `:139` | Solo en la primera instancia |
| `PIPE_TYPE_MESSAGE \| PIPE_READMODE_MESSAGE \| PIPE_WAIT` | — | `:141` | Modo de mensajes, bloqueante |
| `PIPE_UNLIMITED_INSTANCES` | — | `:142` | Sin límite de instancias |
| Tamaños de buffer `65536`, `65536` | salida, entrada | `:142` | — |
| Timeout de `CreateNamedPipe` `0` | — | `:142` | Valor por defecto |
| Atributos de seguridad `_winapi.NULL` | — | `:142` | Se fija después con `restrict_pipe` |
| `PrivateListener(address, 1)` | backlog `1` | `:147` | Un cliente en espera |
| `GetNamedPipeClientProcessId` | — | `:129` | PID del cliente del pipe |

Funciones de `kernel32` declaradas en `kernel()` (`:36-44`, con `argtypes`/`restype`):
`CreateJobObjectW`, `SetInformationJobObject`, `AssignProcessToJobObject`, `TerminateJobObject`,
`QueryInformationJobObject`, `OpenProcess`, `CloseHandle`, `WaitForSingleObject`,
`GetNamedPipeClientProcessId`. `LocalFree` se declara aparte en `restrict_pipe` (`:124`).
Además: `ntdll.NtResumeProcess` (`:104-106`) y `advapi32` (`ConvertStringSecurityDescriptorToSecurityDescriptorW`,
`SetKernelObjectSecurity`, `:117-118`).

### Estructuras (`ctypes`)
- `IO_COUNTERS`: 6 campos `c_ulonglong`: `ReadOperationCount`, `WriteOperationCount`, `OtherOperationCount`, `ReadTransferCount`, `WriteTransferCount`, `OtherTransferCount` (`:8-10`).
- `BASIC_LIMIT`: `PerProcessUserTimeLimit` (`c_longlong`), `PerJobUserTimeLimit` (`c_longlong`), `LimitFlags` (`DWORD`), `MinimumWorkingSetSize` (`c_size_t`), `MaximumWorkingSetSize` (`c_size_t`), `ActiveProcessLimit` (`DWORD`), `Affinity` (`c_size_t`), `PriorityClass` (`DWORD`), `SchedulingClass` (`DWORD`) (`:13-17`).
- `EXTENDED_LIMIT`: `BasicLimitInformation` (`BASIC_LIMIT`), `IoInfo` (`IO_COUNTERS`), `ProcessMemoryLimit`, `JobMemoryLimit`, `PeakProcessMemoryUsed`, `PeakJobMemoryUsed` (todos `c_size_t`) (`:20-23`).
- `BASIC_ACCOUNTING`: `TotalUserTime`, `TotalKernelTime`, `ThisPeriodTotalUserTime`, `ThisPeriodTotalKernelTime` (`c_longlong`); `TotalPageFaultCount`, `TotalProcesses`, `ActiveProcesses`, `TotalTerminatedProcesses` (`DWORD`) (`:26-30`).

## Invariantes
1. **Orden de `spawn`**: el proceso se crea suspendido (`0x4`), se asigna al Job (`:103`) y solo después se reanuda (`:104-108`). Así ningún hijo del comando puede escapar del Job (`:100`: «Assign before user code can start children, then resume the primary thread»).
2. **Kill on close**: el Job tiene `KILL_ON_JOB_CLOSE`; cerrar el handle (`close()`, o liberar el objeto) termina todos sus procesos. Por eso `wait_empty` no es necesario para evitar huérfanos, y `close()` en el `finally` de los llamadores es una garantía de limpieza.
3. **Propiedad tras salir el lanzador**: `wait_empty` espera a que `ActiveProcesses` llegue a 0, no a que el proceso lanzado termine. Un nieto que sigue vivo mantiene el Job ocupado (`test_processes.py:10-19`).
4. **Fallo de `spawn`**: si la asignación o la reanudación falla, el proceso se mata y se espera (`:110-111`); no queda suspendido.
5. **Fallo de creación del Job**: si `SetInformationJobObject` falla, se cierra el handle antes de relanzar (`:75-77`).
6. **Pipe**: solo SYSTEM y el propietario tienen `GA`. Sin `WRITE_DAC` (`0x40000`) no se puede aplicar el DACL (`:138`). Si `restrict_pipe` falla, el handle se cierra antes de relanzar (`:143-145`).
7. `ProcessHandle` y `ProcessJob` son idempotentes en `close()`: un segundo cierre no hace nada.
8. `terminate()` sin handle no hace nada; con handle, falla si `TerminateJobObject` devuelve 0.
9. Los handles no se heredan en los procesos lanzados (docstring, `:1`). **Duda:** el original no pasa `close_fds` ni `handle_list` explícitos; la garantía depende del valor por defecto de `subprocess.Popen` en Windows (`close_fds=True` desde Python 3.7) y de que `**kwargs` del llamador no lo cambie.

## Casos límite y errores
- `checked()` trata `0` y `None` como fallo; `OpenProcess` devuelve `NULL` si el PID ya no existe o no hay permisos → `OSError`.
- `spawn` con `creationflags` explícito **reemplaza** el valor por defecto `CREATE_NO_WINDOW`; `0x4` siempre se añade (`:101`).
- `wait_empty()` sin límite: si un proceso nieto no termina, el llamador se bloquea. Ningún llamador pasa timeout.
- `active_processes()` sobre un Job cerrado: `QueryInformationJobObject(NULL)` falla → `OSError`.
- `pipe_client_pid` espera un objeto con `fileno()`: el handle se obtiene del `Connection` de `multiprocessing`.
- `restrict_pipe` libera el descriptor con `LocalFree` en `finally` (`:121-124`), incluso si `SetKernelObjectSecurity` falla.
- `PrivateListener` sobrescribe el método privado `_new_handle` de `PipeListener`: depende de la implementación interna de CPython. **Duda:** versión de Python en la que existe `PipeListener` con esa firma.

## Casos de test del original

`core/tests/test_processes.py` tiene un único test (solo Windows):

| Test | Qué comprueba | Fase |
|---|---|---|
| `ProcessJobTests.test_wait_empty_keeps_ownership_after_launcher_exits` | Un lanzador (`spawn` de `python -c`) crea un nieto que duerme 1 s y sale. `spawn` devuelve un proceso que termina con 0 (`wait(timeout=10)`). Después `wait_empty()` tarda más de 0,3 s: el Job sigue ocupado aunque el lanzador haya salido. `terminate()` y `close()` en `finally`. | **Duda:** F4 (lanzamiento de apps, `engine.py:675`) y F10 (Pocketool, `pocketool_execution.py:149`). El original no indica fase para este test. |

Nota: el test lanza un intérprete Python con `sys.executable`; en Rust habrá que usar un binario de prueba propio.
Ningún test cubre `ProcessHandle`, `restrict_pipe`, `pipe_listener`, `pipe_client_pid` ni `CREATE_SUSPENDED` directamente.
Los tests de MCP (`test_mcp.py:79-80`, `:250-290`, `:350-351`) cubren `ProcessHandle` de forma indirecta.

Duda: el test no tiene fase asignada en el original; `ProcessJob` lo usan F4 (apps), F8 (host-run), F10 (Pocketool) y F12 (MCP). Confirmar en qué fase debe pasar en Rust.

## Dependencias
- Consumidores de `ProcessJob`: `engine.py:675-676` (lanzamiento de apps), `host_apps.py:126-127`, `jobs.py:261`, `pocketool_execution.py:140-149`, `mcp_service.py:55-63`.
- Consumidores de `ProcessHandle`: `mcp_server.py:56-59` (propietario), `mcp_service.py:87` (`assign=True` para el cliente).
- Consumidores de pipes: `mcp_service.py:55`, `:63` (`pipe_listener`), `:86` (`pipe_client_pid`).
- Tests que parchean o listan el módulo: `test_pocketool_authoring.py:298` (parchea `mito.processes.ProcessJob`); `test_release.py:31` (lista `processes` entre los módulos del release).
- Biblioteca estándar: `ctypes`, `ctypes.wintypes`, `subprocess`, `time`, `_winapi`, `multiprocessing.connection` (solo `pipe_listener`).
- Otros módulos del original no usados aquí: ninguno.

## Notas para Rust

Crate sugerida: `windows` (Microsoft) con las features de `Win32_System_JobObjects`, `Win32_System_Threading`,
`Win32_System_Pipes`, `Win32_Security`, `Win32_Foundation`, `Win32_Security_Authorization` (ConvertStringSecurityDescriptor…).
Alternativa: `windows-sys`, con menos abstracción.

- **Creación suspendida**: `std::process::Command` no permite arrancar suspendido. Hay que usar `CreateProcessW` con `CREATE_SUSPENDED | CREATE_NO_WINDOW`, asignar al Job y luego reanudar. `std::os::windows::process::CommandExt::creation_flags` no sirve para la suspensión.
- **Reanudación**: el original usa `NtResumeProcess` de `ntdll` (`:104-108`), que reanuda todos los hilos del proceso. Con un único hilo inicial es equivalente a `ResumeThread(hThread)`. **Duda:** confirmar que el proceso creado tiene un solo hilo hasta la reanudación; si es así, usar `ResumeThread` (documentado) en vez de `NtResumeProcess`.
- **Herencia de handles**: en Rust hay que asegurar que ningún handle sea heredable, o usar `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` con la lista explícita de stdio. **Duda:** comprobar el comportamiento de la versión de Rust elegida.
- **Job Object**: `CreateJobObjectW`, `SetInformationJobObjectW` con `JobObjectExtendedLimitInformation` y `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Envolver en un tipo con `Drop` que cierre el handle (equivale a `close()`; `Drop` debe cerrar de forma idempotente).
- **`wait_empty`**: el original hace sondeo cada 50 ms (`:95-97`). Para paridad, mantener el sondeo. Una alternativa mejor son los puertos de finalización del Job (`JOBOBJECT_ASSOCIATE_COMPLETION_PORT`), pero cambia el comportamiento temporal y debe anotarse en `docs/PARITY.md` si se adopta.
- **`ProcessHandle`**: `OpenProcess` con `SYNCHRONIZE` (y `PROCESS_SET_QUOTA | PROCESS_TERMINATE` con `assign`); `WaitForSingleObject(h, 0) == WAIT_TIMEOUT` para `alive()`.
- **Named pipe**: `CreateNamedPipeW` con `PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | WRITE_DAC` (+ `FILE_FLAG_FIRST_PIPE_INSTANCE` en la primera instancia), `PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT`, `PIPE_UNLIMITED_INSTANCES`, buffers 65536. Aplicar el SDDL con `ConvertStringSecurityDescriptorToSecurityDescriptorW(…, SDDL_REVISION_1, …)` y `SetKernelObjectSecurity(…, DACL_SECURITY_INFORMATION, …)`. Liberar con `LocalFree`.
- **PID del cliente**: `GetNamedPipeClientProcessId` sobre el handle del pipe. La API de `std` no la expone; usar `windows` directamente.
- **Errores**: `windows::core::Error::from_win32()` equivale a `WinError(GetLastError())`. Mapear a un error de `mito-win` con `thiserror`, que se envuelve en `mito_core::Error` (ADR 0008, punto 7).
- **Sin `ctypes`**: las estructuras `EXTENDED_LIMIT`, `BASIC_ACCOUNTING`, etc., las da el crate `windows`. Verificar que los tamaños coinciden (en particular `IoInfo` y `c_size_t`).
- **`PrivateListener`**: no hay equivalente directo a `multiprocessing.connection` en Rust. Hay que implementar el listener de named pipes a mano (instancia nueva por cliente, `ConnectNamedPipe`). **Duda:** si el protocolo de mensajes de `multiprocessing` (longitud + payload) debe reproducirse para compatibilidad con clientes Python. Probablemente no, porque el cliente también será Rust.
- **Duda:** `ProcessJob.spawn` acepta `**kwargs` (`stdin`, `stdout`, `cwd`, `env`, etc.) que pasan a `Popen`. La crate de Rust debe tener una API equivalente, quizá una struct de opciones.
- **Duda:** el test original requiere un intérprete Python. El equivalente Rust necesitará un binario de prueba (`examples/` o un bin de `tests/`) que lance un nieto. Marcar `#[cfg(windows)]`.
