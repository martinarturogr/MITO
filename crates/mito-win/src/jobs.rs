//! Job Objects para el tiempo de vida de los procesos lanzados (`ProcessJob` de
//! `processes.py`).
//!
//! Un lanzamiento `--owned` crea un Job con `KILL_ON_JOB_CLOSE`, arranca el comando
//! suspendido, lo asigna al Job **antes** de que ejecute código y lo reanuda. Así ningún
//! hijo puede escapar del Job, y cerrar el Job termina todo el árbol.

use std::io;
use std::os::windows::io::AsRawHandle as _;
use std::os::windows::process::CommandExt as _;
use std::process::{Child, Command};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::core::{PCWSTR, s};

use crate::registry::wide;

/// `CREATE_SUSPENDED`.
const CREATE_SUSPENDED: u32 = 0x0000_0004;

fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xffff)
}

type ResumeProcess = unsafe extern "system" fn(HANDLE) -> i32;

/// `NtResumeProcess` de `ntdll.dll` (reanuda todos los hilos, como el original).
fn resume_process() -> io::Result<ResumeProcess> {
    static RESUME: OnceLock<Option<usize>> = OnceLock::new();
    let address = RESUME.get_or_init(|| {
        let name = wide("ntdll.dll");
        // SAFETY: `ntdll.dll` está cargada en todo proceso de Windows; `name` es UTF-16
        // terminado en NUL.
        let module = unsafe { GetModuleHandleW(PCWSTR(name.as_ptr())) }.ok()?;
        // SAFETY: `module` es ntdll y el nombre es ASCII terminado en NUL.
        unsafe { GetProcAddress(module, s!("NtResumeProcess")) }.map(|f| f as usize)
    });
    let address = address.ok_or_else(|| io::Error::other("Could not resume command"))?;
    // SAFETY: la dirección es la de `NtResumeProcess`, cuya firma es
    // `NTSTATUS NtResumeProcess(HANDLE)`.
    Ok(unsafe { std::mem::transmute::<usize, ResumeProcess>(address) })
}

/// Job anónimo con `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Al soltarlo (o con
/// [`ProcessJob::close`]) mueren todos sus procesos.
#[derive(Debug)]
pub struct ProcessJob {
    handle: Option<HANDLE>,
}

// SAFETY: el handle de un Job es un objeto del núcleo que puede usarse desde cualquier
// hilo; `ProcessJob` no comparte estado mutable sin sincronizar.
unsafe impl Send for ProcessJob {}
// SAFETY: como arriba; las funciones de Job son seguras entre hilos.
unsafe impl Sync for ProcessJob {}

impl ProcessJob {
    /// Crea el Job.
    ///
    /// # Errors
    ///
    /// El error del sistema si no se puede crear o configurar (se cierra antes).
    pub fn new() -> io::Result<Self> {
        // SAFETY: sin atributos ni nombre; el resultado se comprueba.
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|e| os_error(&e))?;
        let job = Self {
            handle: Some(handle),
        };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(size_of_val(&limits)).unwrap_or(u32::MAX);
        // SAFETY: `handle` es el Job recién creado; `limits` es una estructura local del
        // tamaño indicado que la función solo lee.
        unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            )
        }
        .map_err(|e| os_error(&e))?;
        Ok(job)
    }

    fn handle(&self) -> io::Result<HANDLE> {
        self.handle
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "Job cerrado"))
    }

    /// Asigna al Job el proceso de `child`.
    ///
    /// # Errors
    ///
    /// El error del sistema (p. ej. si el proceso ya está en un Job sin *breakaway*).
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        let process = HANDLE(child.as_raw_handle());
        // SAFETY: el Job está abierto y `process` es el handle vivo de `child`.
        unsafe { AssignProcessToJobObject(self.handle()?, process) }.map_err(|e| os_error(&e))
    }

    /// Arranca `command` suspendido con `flags | CREATE_SUSPENDED`, lo asigna al Job y lo
    /// reanuda (`spawn`). Si falla la asignación o la reanudación, mata el proceso.
    ///
    /// `flags` sustituye a los de `command`, como `creationflags` en el original.
    ///
    /// # Errors
    ///
    /// Los de `spawn`, la asignación y `Could not resume command`.
    pub fn spawn(&self, command: &mut Command, flags: u32) -> io::Result<Child> {
        command.creation_flags(flags | CREATE_SUSPENDED);
        let mut child = command.spawn()?;
        let started = self.assign(&child).and_then(|()| {
            let resume = resume_process()?;
            // SAFETY: `child` sigue vivo y su handle tiene `PROCESS_SUSPEND_RESUME`
            // (handle completo devuelto por `CreateProcessW`).
            let status = unsafe { resume(HANDLE(child.as_raw_handle())) };
            if status == 0 {
                Ok(())
            } else {
                Err(io::Error::other("Could not resume command"))
            }
        });
        if let Err(error) = started {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(child)
    }

    /// Procesos vivos del Job (`ActiveProcesses`).
    ///
    /// # Errors
    ///
    /// El error del sistema si la consulta falla.
    pub fn active_processes(&self) -> io::Result<u32> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let size = u32::try_from(size_of_val(&info)).unwrap_or(u32::MAX);
        // SAFETY: el Job está abierto; `info` es una estructura local del tamaño indicado
        // que la función escribe.
        unsafe {
            QueryInformationJobObject(
                Some(self.handle()?),
                JobObjectBasicAccountingInformation,
                (&raw mut info).cast(),
                size,
                None,
            )
        }
        .map_err(|e| os_error(&e))?;
        Ok(info.ActiveProcesses)
    }

    /// Espera, sondeando cada `interval`, a que no quede ningún proceso en el Job
    /// (`wait_empty`). Sin tiempo máximo, como el original.
    ///
    /// # Errors
    ///
    /// Los de [`ProcessJob::active_processes`].
    pub fn wait_empty(&self, interval: Duration) -> io::Result<()> {
        while self.active_processes()? > 0 {
            thread::sleep(interval);
        }
        Ok(())
    }

    /// Termina todos los procesos del Job con código 1. Sin Job, no hace nada.
    ///
    /// # Errors
    ///
    /// El error del sistema.
    pub fn terminate(&self) -> io::Result<()> {
        let Some(handle) = self.handle else {
            return Ok(());
        };
        // SAFETY: el Job está abierto.
        unsafe { TerminateJobObject(handle, 1) }.map_err(|e| os_error(&e))
    }

    /// Cierra el Job, lo que termina sus procesos. Idempotente.
    pub fn close(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: `handle` es el Job abierto por `new` y se cierra una sola vez.
            let _ = unsafe { CloseHandle(handle) };
        }
    }
}

impl Drop for ProcessJob {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// `CREATE_NO_WINDOW`.
    const NO_WINDOW: u32 = 0x0800_0000;

    #[test]
    fn el_job_sigue_ocupado_tras_salir_el_lanzador() {
        let job = ProcessJob::new().unwrap();
        // El lanzador sale enseguida y deja un nieto (`ping`) vivo unos segundos.
        let mut command = Command::new("cmd.exe");
        command.args(["/d", "/c", "start", "/b", "ping", "-n", "3", "127.0.0.1"]);
        command.stdout(std::process::Stdio::null());
        let mut child = job.spawn(&mut command, NO_WINDOW).unwrap();
        assert!(child.wait().unwrap().success());
        let start = Instant::now();
        assert!(job.active_processes().unwrap() >= 1);
        job.wait_empty(Duration::from_millis(50)).unwrap();
        assert!(start.elapsed() > Duration::from_millis(300));
        assert_eq!(job.active_processes().unwrap(), 0);
    }

    #[test]
    fn cerrar_el_job_mata_el_arbol() {
        let mut job = ProcessJob::new().unwrap();
        let mut command = Command::new("ping");
        command
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null());
        let mut child = job.spawn(&mut command, NO_WINDOW).unwrap();
        let start = Instant::now();
        job.close();
        job.close();
        // `ping -n 30` tardaría 29 s; al cerrar el Job termina enseguida (el código de
        // salida de un proceso terminado por el Job es 0).
        child.wait().unwrap();
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(job.active_processes().is_err());
        job.terminate().unwrap();
    }

    #[test]
    fn un_ejecutable_inexistente_no_deja_procesos() {
        let job = ProcessJob::new().unwrap();
        let mut command = Command::new(r"C:\no\existe\mito-nada.exe");
        assert!(job.spawn(&mut command, NO_WINDOW).is_err());
        assert_eq!(job.active_processes().unwrap(), 0);
    }
}
