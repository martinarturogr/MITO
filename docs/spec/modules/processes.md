# processes.py → mito-win::processes (crate `mito-win`, consumido por `mito-engine`)

Origen: `core/app/mito/processes.py` (147 líneas), `core/tests/test_processes.py` (26 líneas, 1 test). Cobertura indirecta: `core/tests/test_shortcuts.py:169-177` (`test_owned_launch_assigns_before_running_and_waits_for_job_tree`, con `ProcessJob` simulado) y `core/tests/test_cli.py:50-109` (`--owned` y pausa).

## Responsabilidad

Gestionar el tiempo de vida de procesos en Windows con **Job Objects**. Un lanzamiento `--owned` crea un Job, arranca el comando suspendido, lo asigna al Job, lo reanuda y espera a que el árbol del Job quede vacío. Además: abrir un proceso por PID para vigilarlo (`ProcessHandle`) y crear un named pipe que solo puedan usar su propietario y SYSTEM, identificando al cliente por PID (MCP, F12). El docstring indica «Handles are never inherited by launched commands» (`processes.py:1`).

El modo acoplado (`--coupled`) **no** usa este módulo directamente: abre una pestaña de la terminal de MITO (ver «Modo acoplado»).

## Consumidores

- `ProcessJob`: `engine.py:675-684` (`run --owned`), `host_apps.py:125-134` (`host-run --owned`), `jobs.py:16` y `:261` (trabajos de la UI), `pocketool_execution.py:140-149` (F10), `mcp_service.py:55-63` (F12).
- `ProcessHandle`: `mcp_server.py:56-59` (propietario del MCP), `mcp_service.py:87` (`assign=True` para el cliente).
- Pipes: `mcp_service.py:55`, `:63` (`pipe_listener`), `:86` (`pipe_client_pid`).
- Tests que parchean o listan el módulo: `test_pocketool_authoring.py:298` (parchea `mito.processes.ProcessJob`); `test_release.py:31` (lista `processes` entre los módulos del release). Los tests de MCP (`test_mcp.py:79-80`, `:250-290`, `:350-351`) cubren `ProcessHandle` de forma indirecta.

## API pública del original

| Función/clase | Entradas | Salida | Errores |
|---|---|---|---|
| `class ProcessHandle(pid, assign=False)` (`:56-66`) | PID; si `assign` pide también `PROCESS_SET_QUOTA` y `PROCESS_TERMINATE` | objeto con `handle` | `OSError` (`WinError`) si `OpenProcess` falla |
| `ProcessHandle.alive() -> bool` (`:61-62`) | — | `True` si `WaitForSingleObject(h, 0) == 258` | — |
| `ProcessHandle.close()` (`:64-66`) | — | — | Idempotente (`if self.handle`) |
| `class ProcessJob()` (`:69-111`) | — | Job anónimo con `KILL_ON_JOB_CLOSE` | `OSError` si `CreateJobObjectW` o `SetInformationJobObject` fallan (en el segundo caso cierra el handle y relanza) |
| `ProcessJob.assign(handle)` (`:79-80`) | handle de proceso (`int`) | — | `OSError` si `AssignProcessToJobObject` falla |
| `ProcessJob.terminate()` (`:82-84`) | — | — | `OSError` si `TerminateJobObject` falla; no hace nada sin handle |
| `ProcessJob.close()` (`:86-88`) | — | — | Idempotente. **Cerrar el Job mata a todos sus procesos** |
| `ProcessJob.active_processes() -> int` (`:90-93`) | — | `ActiveProcesses` de `JobObjectBasicAccountingInformation` | `OSError` si la consulta falla |
| `ProcessJob.wait_empty(interval=.05)` (`:95-97`) | intervalo en segundos | — | Bloquea mientras `active_processes() > 0`. Sin tiempo máximo |
| `ProcessJob.spawn(argv, creationflags=subprocess.CREATE_NO_WINDOW, **kwargs)` (`:99-111`) | argumentos, flags adicionales, resto de kwargs de `Popen` | `subprocess.Popen` ya reanudado | `OSError('Could not resume command')` si `NtResumeProcess` falla; relanza cualquier error tras matar el proceso |
| `restrict_pipe(handle)` (`:114-124`) | handle de named pipe | — | `OSError` si la conversión SDDL o `SetKernelObjectSecurity` fallan |
| `pipe_client_pid(connection) -> int` (`:127-130`) | objeto con `fileno()` (conexión de `multiprocessing`) | PID del cliente | `OSError` si `GetNamedPipeClientProcessId` falla |
| `pipe_listener(address) -> PrivateListener` (`:133-147`) | dirección `\\.\pipe\…` | listener de `multiprocessing.connection` con `backlog` 1 y `_new_handle` propio | `OSError` si `CreateNamedPipe` o `restrict_pipe` fallan (cierra el handle y relanza) |
| `kernel()` (`:33-47`) | — | `WinDLL('kernel32')` con `argtypes`/`restype` fijados | — |
| `checked(value)` (`:50-53`) | valor devuelto | el mismo valor | `OSError` con `WinError(GetLastError())` si `value` es 0/NULL |

### Estructuras `ctypes` (referencia de layout)

- `IO_COUNTERS` (`:8-10`): 6 campos `c_ulonglong`: `ReadOperationCount`, `WriteOperationCount`, `OtherOperationCount`, `ReadTransferCount`, `WriteTransferCount`, `OtherTransferCount`.
- `BASIC_LIMIT` (`:13-17`): `PerProcessUserTimeLimit` (`c_longlong`), `PerJobUserTimeLimit` (`c_longlong`), `LimitFlags` (`DWORD`), `MinimumWorkingSetSize` (`c_size_t`), `MaximumWorkingSetSize` (`c_size_t`), `ActiveProcessLimit` (`DWORD`), `Affinity` (`c_size_t`), `PriorityClass` (`DWORD`), `SchedulingClass` (`DWORD`).
- `EXTENDED_LIMIT` (`:20-23`): `BasicLimitInformation` (`BASIC_LIMIT`), `IoInfo` (`IO_COUNTERS`), `ProcessMemoryLimit`, `JobMemoryLimit`, `PeakProcessMemoryUsed`, `PeakJobMemoryUsed` (todos `c_size_t`).
- `BASIC_ACCOUNTING` (`:26-30`): `TotalUserTime`, `TotalKernelTime`, `ThisPeriodTotalUserTime`, `ThisPeriodTotalKernelTime` (`c_longlong`); `TotalPageFaultCount`, `TotalProcesses`, `ActiveProcesses`, `TotalTerminatedProcesses` (`DWORD`).

## Formatos de disco

Ninguno. El módulo no lee ni escribe archivos. El nombre del pipe lo elige el llamador.

## Constantes Win32 y valores literales

| Constante / valor | Valor | Dónde | Significado |
|---|---|---|---|
| `OpenProcess` access `0x100000` | `SYNCHRONIZE` | `:59` | Permite `WaitForSingleObject` sobre el proceso |
| `OpenProcess` access `0x101` | `PROCESS_SET_QUOTA` (0x100) \| `PROCESS_TERMINATE` (0x1) | `:59` | Necesario para `AssignProcessToJobObject`; solo con `assign=True` |
| `WaitForSingleObject` → `258` | `WAIT_TIMEOUT` | `:62` | El proceso sigue vivo |
| `WaitForSingleObject(h, 0)` | timeout 0 | `:62` | Sondeo sin bloquear |
| `SetInformationJobObject` clase `9` | `JobObjectExtendedLimitInformation` | `:75` | Se pasa `EXTENDED_LIMIT` |
| `LimitFlags = 0x2000` | `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` | `:73` | Al cerrar el último handle del Job mueren sus procesos |
| `QueryInformationJobObject` clase `1` | `JobObjectBasicAccountingInformation` | `:92` | Campo `ActiveProcesses` |
| `TerminateJobObject(handle, 1)` | código de salida `1` | `:84` | Mata todos los procesos del Job |
| `creationflags` de `spawn` | `0x4` = `CREATE_SUSPENDED`, sumado a `creationflags` (por defecto `CREATE_NO_WINDOW`) | `:101` | El proceso empieza suspendido |
| `subprocess.CREATE_NO_WINDOW` | `0x08000000` | `:99` (defecto) | Sin ventana de consola |
| `NtResumeProcess` (ntdll) | `c_long` de retorno; `0` = éxito | `:104-108` | Reanuda el proceso (no documentada oficialmente) |
| `ConvertStringSecurityDescriptorToSecurityDescriptorW` revisión `1` | `SDDL_REVISION_1` | `:120` | — |
| SDDL `D:P(A;;GA;;;SY)(A;;GA;;;OW)` | DACL protegida; `GA` para `SY` (SYSTEM) y `OW` (propietario) | `:120` | Única ACE para SYSTEM y propietario |
| `SetKernelObjectSecurity` `4` | `DACL_SECURITY_INFORMATION` | `:122` | Se aplica solo el DACL |
| `_winapi.PIPE_ACCESS_DUPLEX`, `FILE_FLAG_OVERLAPPED` | — | `:138` | Pipe bidireccional con E/S superpuesta |
| `0x40000` | `WRITE_DAC` (`0x00040000`) | `:138` | Permite cambiar el DACL del pipe; sin él `SetKernelObjectSecurity` falla |
| `_winapi.FILE_FLAG_FIRST_PIPE_INSTANCE` | — | `:139` | Solo en la primera instancia |
| `PIPE_TYPE_MESSAGE \| PIPE_READMODE_MESSAGE \| PIPE_WAIT` | — | `:141` | Modo de mensajes, bloqueante |
| `PIPE_UNLIMITED_INSTANCES`, buffers `65536`/`65536` | — | `:142` | Sin límite de instancias |
| Atributos de seguridad `_winapi.NULL` | — | `:142` | Se fijan después con `restrict_pipe` |
| `PrivateListener(address, 1)` | backlog `1` | `:147` | Un cliente en espera |
| `GetNamedPipeClientProcessId` | — | `:129` | PID del cliente del pipe |

Funciones de `kernel32` declaradas en `kernel()` (`:36-44`): `CreateJobObjectW`, `SetInformationJobObject`, `AssignProcessToJobObject`, `TerminateJobObject`, `QueryInformationJobObject`, `OpenProcess`, `CloseHandle`, `WaitForSingleObject`, `GetNamedPipeClientProcessId`. `LocalFree` se declara aparte en `restrict_pipe` (`:124`). Además `ntdll.NtResumeProcess` (`:104-106`) y `advapi32` (`ConvertStringSecurityDescriptorToSecurityDescriptorW`, `SetKernelObjectSecurity`, `:117-118`).

## Algoritmo de `ProcessJob.spawn` (orden exacto)

1. `subprocess.Popen(argv, creationflags=0x4 | creationflags, **kwargs)`. El proceso nace suspendido. Por defecto `creationflags = CREATE_NO_WINDOW`.
2. `self.assign(process._handle)`: el proceso entra en el Job **antes** de ejecutar código del usuario.
3. `ntdll.NtResumeProcess(process._handle)`: si devuelve distinto de 0, `OSError('Could not resume command')`.
4. Devuelve el `Popen`.
5. Cualquier excepción tras el `Popen`: `process.kill()`, `process.wait()` y se relanza. El proceso no queda suspendido.

`engine.py` y `host_apps.py` pasan `creationflags=0` explícitamente (el test de `shortcuts` lo verifica), así que el resultado es `0x4` sin `CREATE_NO_WINDOW`: la pestaña de Windows Terminal ya proporciona consola.

## Modo acoplado (`--coupled`) frente a `--owned`

- `--owned`: el proceso de la pestaña lanza el comando con `ProcessJob`, espera `process.wait()` y después `wait_empty()`. Devuelve el código del **proceso lanzador**, no del árbol. Después la CLI pausa con `pause_owned_exit`.
- `--coupled`: `terminal.launch_component` / `terminal.launch_host` abren con `Popen` una pestaña nueva en la ventana de Windows Terminal de MITO (`-w <ventana> new-tab`) cuyo proceso es `mito run --owned …` (o `host-run --owned …`). El manager no espera: `Popen` devuelve de inmediato, y por eso `run --coupled` devuelve 0.
- Así, el modo acoplado depende de `ProcessJob` dentro de la pestaña, pero el código que lo arranca vive en `terminal.py`.

## Invariantes

1. **Orden de `spawn`**: suspendido (`0x4`) → asignado al Job → reanudado. Ningún hijo puede escapar del Job (`:100`: «Assign before user code can start children, then resume the primary thread»).
2. **Kill on close**: el Job tiene `KILL_ON_JOB_CLOSE`; cerrar el último handle termina todos sus procesos. `close()` en el `finally` de los llamadores garantiza la limpieza aunque `wait_empty` no llegue a ejecutarse.
3. **Propiedad tras salir el lanzador**: `wait_empty` espera a que `ActiveProcesses` llegue a 0, no a que el lanzador termine. Un nieto vivo mantiene el Job ocupado (`test_processes.py:10-19`).
4. **Fallo de `spawn`**: si la asignación o la reanudación falla, el proceso se mata y se espera (`:110-111`).
5. **Fallo de creación del Job**: si `SetInformationJobObject` falla, se cierra el handle antes de relanzar (`:75-77`).
6. **Pipe**: solo SYSTEM y el propietario tienen `GA`. Sin `WRITE_DAC` (`0x40000`) no se puede aplicar el DACL (`:138`). Si `restrict_pipe` falla, el handle se cierra antes de relanzar (`:143-145`).
7. `ProcessHandle` y `ProcessJob` son idempotentes en `close()`. `terminate()` sin handle no hace nada.
8. `wait_empty` no devuelve mientras quede algún proceso del Job. `TerminateJobObject` solo se llama desde `terminate()`, nunca en el flujo normal.
9. Los handles no se heredan en los procesos lanzados (docstring, `:1`). La garantía depende de `close_fds=True`, valor por defecto de `subprocess.Popen` en Windows desde Python 3.7, y de que el llamador no pase `close_fds` ni `handle_list` en `**kwargs`.

## Casos límite y errores

- `checked()` trata `0` y `None` como fallo. `OpenProcess` devuelve `NULL` si el PID ya no existe o no hay permisos → `OSError`.
- `spawn` con `creationflags` explícito **reemplaza** el valor por defecto `CREATE_NO_WINDOW`; `0x4` siempre se añade.
- `wait_empty()` sin límite: si un nieto no termina, el llamador se bloquea. Ningún llamador pasa timeout.
- `active_processes()` sobre un Job cerrado: `QueryInformationJobObject(NULL)` falla → `OSError`.
- `pipe_client_pid` espera un objeto con `fileno()`; se obtiene del `Connection` de `multiprocessing`.
- `restrict_pipe` libera el descriptor con `LocalFree` en `finally` (`:121-124`), incluso si `SetKernelObjectSecurity` falla.
- `PrivateListener` sobrescribe el método privado `_new_handle` de `PipeListener`: depende de la implementación interna de CPython.
- Un proceso que ya pertenece a otro Job que no permite *breakaway* hace fallar `AssignProcessToJobObject`. `spawn` mata el proceso y relanza el error; el original no lo distingue de otros fallos.
- Ejecutable inexistente: `Popen` falla antes de crear el proceso; el `close` del `finally` del llamador limpia el Job.
- Ctrl+C durante `wait`: el `finally` del llamador cierra el Job y mata el árbol.

## Casos de test del original

`core/tests/test_processes.py` tiene un único test (solo Windows):

| Test | Qué comprueba |
|---|---|
| `ProcessJobTests.test_wait_empty_keeps_ownership_after_launcher_exits` (`:10-22`) | Un lanzador (`spawn` de `python -c`) crea un nieto que duerme 1 s y sale. `spawn` devuelve un proceso que termina con 0 (`wait(timeout=10)`). Después `wait_empty()` tarda más de 0,3 s: el Job sigue ocupado aunque el lanzador haya salido. `terminate()` y `close()` en `finally`. |

Notas sobre los tests:
- El test lanza un intérprete Python con `sys.executable`; en Rust hará falta un binario de prueba propio (`#[cfg(windows)]`).
- Ningún test cubre `ProcessHandle`, `restrict_pipe`, `pipe_listener`, `pipe_client_pid` ni `CREATE_SUSPENDED` directamente. Los de MCP los cubren indirectamente.
- `test_owned_launch_assigns_before_running_and_waits_for_job_tree` (`test_shortcuts.py:169-177`) verifica `spawn` con `creationflags=0`, `wait_empty` y `close` con `ProcessJob` simulado.

## Dependencias

- `ctypes`, `ctypes.wintypes`, `subprocess`, `time`, `_winapi`, `multiprocessing.connection` (solo `pipe_listener`).
- Sin `storage` ni `i18n`: los errores son `WinError` o `OSError('Could not resume command')` sin traducir.

## Estado en Rust

- `crates/mito-engine/src/launch.rs` (ya portado): `launch_command_from`, `LaunchOptions` (`coupled`, `owned`, `shortcut`), `LaunchCommand::to_command` (`env_clear` y `current_dir`), `batch_command` (línea de comandos cruda para `cmd /c`), `CREATE_NEW_CONSOLE` y `new_terminal`. Los errores `Modo de lanzamiento no válido` están portados.
- `crates/mito-engine/src/extensions.rs`: el trait `Jobs` (`owned` y `coupled`) existe, pero su implementación por defecto `NoJobs` devuelve `Modo de lanzamiento no soportado todavía: --owned` o `--coupled` (`crates/mito/tests/cli.rs:689-702`).
- `crates/mito-terminal/src/command.rs`: `launch_application`, `launch_component` y `launch_host` (lado de la pestaña) ya están portados.
- **Falta**: `ProcessJob` completo, `ProcessHandle` y las utilidades de pipe para F12. `mito-win` no tiene nada de esto (`links.rs`, `taskbar.rs`, `resources.rs`, `registry.rs`, `console.rs`, `clipboard.rs`, `files.rs`, `system.rs`, `msi.rs`).
- Diferencia: el Python usa `CREATE_NEW_CONSOLE` para `.bat`/`.cmd`/`.ps1` en la ruta independiente de `host_apps`. `launch.rs` solo lo aplica con `new_terminal`.
- PARITY marca `processes.py` como 🟨 (F4 para lanzamiento, F8 para Job Objects).

## Notas para Rust

- Crate `windows` (ya en el workspace). Hay que añadir features que faltan: `Win32_System_JobObjects` (no está en la lista actual del `Cargo.toml` raíz), `Win32_System_Pipes`, `Win32_Security_Authorization` (para `ConvertStringSecurityDescriptorToSecurityDescriptorW`). Ya están `Win32_Security`, `Win32_Foundation`, `Win32_System_Threading`.
- **Creación suspendida**: `std::process::Command` no permite arrancar suspendido. Usar `CreateProcessW` con `CREATE_SUSPENDED | CREATE_NO_WINDOW`, asignar al Job y reanudar. `CommandExt::creation_flags` no sirve para la suspensión.
- **Reanudación**: el original usa `NtResumeProcess` (`:104-108`), que reanuda todos los hilos. Con un único hilo inicial es equivalente a `ResumeThread(hThread)`, que está documentado. **Duda**: confirmar que el proceso creado tiene un solo hilo hasta la reanudación; si es así, usar `ResumeThread`.
- **Herencia de handles**: asegurar que ningún handle sea heredable, o usar `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` con la lista explícita de stdio.
- **Job Object**: `CreateJobObjectW` y `SetInformationJobObject` con `JobObjectExtendedLimitInformation` y `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Envolver en un tipo con `Drop` que cierre el handle (equivale a `close()`, idempotente).
- **`wait_empty`**: el original sondea cada 50 ms (`:95-97`). Para paridad, mantener el sondeo. Los puertos de finalización del Job (`JOBOBJECT_ASSOCIATE_COMPLETION_PORT`) serían mejores, pero cambian el comportamiento temporal: anotar en `docs/PARITY.md` si se adoptan.
- **`ProcessHandle`**: `OpenProcess` con `SYNCHRONIZE` (y `PROCESS_SET_QUOTA | PROCESS_TERMINATE` con `assign`); `alive()` = `WaitForSingleObject(h, 0) == WAIT_TIMEOUT`.
- **Named pipe**: `CreateNamedPipeW` con `PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | WRITE_DAC` (y `FILE_FLAG_FIRST_PIPE_INSTANCE` en la primera), `PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT`, `PIPE_UNLIMITED_INSTANCES`, buffers 65536. SDDL con `SDDL_REVISION_1` y `SetKernelObjectSecurity(…, DACL_SECURITY_INFORMATION, …)`. Liberar con `LocalFree`.
- **PID del cliente**: `GetNamedPipeClientProcessId` sobre el handle del pipe. `std` no lo expone; usar `windows` directamente.
- **Errores**: `windows::core::Error::from_win32()` equivale a `WinError(GetLastError())`. Mapear a un error de `mito-win` con `thiserror` (ADR 0008, punto 7).
- **Sin `ctypes`**: las estructuras de Job las da el crate `windows` (`JOBOBJECT_EXTENDED_LIMIT_INFORMATION`, `JOBOBJECT_BASIC_ACCOUNTING_INFORMATION`). No declararlas a mano.
- **`PrivateListener`**: no hay equivalente en Rust. Implementar el listener de named pipes a mano (instancia nueva por cliente, `ConnectNamedPipe`). **Duda**: si el protocolo de `multiprocessing` (longitud + payload) debe reproducirse; probablemente no, porque el cliente también será Rust.
- **`spawn` con `**kwargs`**: el original pasa `stdin`, `stdout`, `cwd`, `env`… a `Popen`. La API Rust necesita una struct de opciones equivalente.
- Propuesta de ubicación: `ProcessJob` debe implementar `mito_engine::extensions::Jobs` (en `mito-win` o en el binario) y sustituir `NoJobs` en el arranque.

## Dudas

1. `wait_empty` no tiene timeout. Si un nieto queda colgado, la pestaña `--owned` no se cierra nunca. ¿Es el comportamiento esperado?
2. El código devuelto por `run --owned` es el del lanzador, no el del árbol. Si el lanzador sale con 0 y un nieto falla, MITO muestra 0. ¿Es intencional?
3. `process._handle` es un atributo privado de `Popen`. En Rust no hay equivalente directo con `std::process::Command`: hay que decidir el mecanismo (`CreateProcessW` directo).
4. `ProcessHandle.alive()` no se usa en la ruta de la CLI; solo lo usa `mcp_server.py:56-59`. Confirmar que sigue siendo necesario en F12.
5. Si MITO corre dentro de un Job (lanzado desde un instalador o un CI), `AssignProcessToJobObject` puede fallar. El original no lo comprueba ni reintenta.
6. `TerminateJobObject` nunca se llama en el flujo de `--owned`, solo en `ProcessJob.terminate()`. ¿Hace falta un camino de cancelación (Ctrl+C) que lo use?
7. El test no tiene fase asignada en el original; `ProcessJob` lo usan F4 (apps), F8 (host-run), F10 (Pocketool) y F12 (MCP). Confirmar en qué fase debe pasar en Rust.
8. `PrivateListener` depende de `PipeListener._new_handle`, un método privado de CPython. Confirmar la versión de Python que lo tiene con esa firma.
