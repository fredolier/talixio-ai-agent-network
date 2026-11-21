//! Ce que la sécurité demande au système d'exploitation, sous une seule interface :
//! fichiers fermés aux autres comptes, ouverture sans suivre de lien, process fermé aux
//! autres, et rien de ce qu'un agent lance qui lui survive.
//!
//! Unix et Windows n'offrent pas les mêmes mécanismes ; chaque fonction a donc deux
//! écritures, qui doivent tenir la même promesse.

use std::fs::{File, OpenOptions};
use std::path::Path;

use tokio::process::{Child, Command};

/// Ce qui désigne une commande et tout ce qu'elle a lancé : un groupe de process sous Unix,
/// un « Job Object » sous Windows. Zéro : rien.
pub type Group = i64;

/// Droits d'un fichier ou d'un dossier secret qu'on crée : le compte de l'agent, et lui seul.
pub fn make_private(path: &Path) -> Result<(), String> {
    imp::make_private(path).map_err(|e| format!("{} : {e}", path.display()))
}

/// Un fichier secret doit être à nous, et fermé aux autres comptes.
pub fn check_private(path: &Path) -> Result<(), String> {
    imp::check_private(path)
}

/// Options d'ouverture qui ne suivent pas un lien en bout de chemin, et ne restent pas
/// bloquées sur un tube.
pub fn no_follow(options: &mut OpenOptions) {
    imp::no_follow(options)
}

/// Le fichier ouvert est-il bien celui que désigne, maintenant, le chemin vérifié ? Entre la
/// vérification et l'ouverture, un dossier du chemin a pu être remplacé par un lien.
pub fn same_file(open: &File, path: &Path) -> Result<(), String> {
    match (imp::identity(open), imp::identity_of(path)) {
        (Ok(a), Ok(b)) if a.0 == b.0 => Ok(()),
        (Ok(_), Ok(_)) => Err("accès refusé : le chemin a changé pendant l'opération".into()),
        (Err(e), _) | (_, Err(e)) => Err(e.to_string()),
    }
}

/// Nombre de noms du fichier ouvert : plus d'un, et c'est un lien physique vers ailleurs.
pub fn link_count(open: &File) -> Result<u64, String> {
    imp::identity(open).map(|(_, links)| links).map_err(|e| e.to_string())
}

/// Ferme la mémoire du process aux autres process du compte, et fait en sorte que tout ce
/// qu'il lancera meure avec lui.
pub fn harden_process() {
    imp::harden_process()
}

/// Prépare une commande pour qu'elle parte dans son propre groupe.
pub fn isolate(command: &mut Command) {
    imp::isolate(command)
}

/// À appeler juste après le lancement d'une commande préparée par `isolate` : la range dans
/// son groupe et la laisse démarrer. Si le groupe ne peut pas être formé, la commande est tuée.
pub fn adopt(child: &mut Child) -> Result<Group, String> {
    imp::adopt(child)
}

/// Tue le groupe entier : la commande et tout ce qu'elle a lancé.
pub fn kill_group(group: Group) {
    imp::kill_group(group)
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

    pub fn make_private(path: &Path) -> io::Result<()> {
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    pub fn check_private(path: &Path) -> Result<(), String> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{} : {e}", path.display()))?;
        // SAFETY: geteuid ne fait que lire l'identité du process.
        let me = unsafe { libc::geteuid() };
        if meta.uid() != me {
            return Err(format!("{} appartient à un autre compte", path.display()));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(format!("{} est lisible par d'autres comptes : chmod 600", path.display()));
        }
        Ok(())
    }

    pub fn no_follow(options: &mut OpenOptions) {
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }

    pub fn identity(open: &File) -> io::Result<((u64, u64), u64)> {
        let meta = open.metadata()?;
        Ok(((meta.dev(), meta.ino()), meta.nlink()))
    }

    pub fn identity_of(path: &Path) -> io::Result<((u64, u64), u64)> {
        let meta = std::fs::symlink_metadata(path)?;
        Ok(((meta.dev(), meta.ino()), meta.nlink()))
    }

    pub fn harden_process() {
        #[cfg(target_os = "linux")]
        // SAFETY: simple appel système, sans pointeur.
        unsafe {
            libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
        }
        #[cfg(target_os = "macos")]
        // SAFETY: simple appel système, sans pointeur. Interdit à tout autre process de s'attacher.
        unsafe {
            libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0);
        }
    }

    pub fn isolate(command: &mut Command) {
        command.process_group(0);
        #[cfg(target_os = "linux")]
        // SAFETY: entre fork et exec, un seul appel système sans allocation. Si l'agent meurt
        // brutalement, la commande est tuée par le noyau.
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    pub fn adopt(child: &mut Child) -> Result<Group, String> {
        Ok(child.id().map(Group::from).unwrap_or(0))
    }

    pub fn kill_group(group: Group) {
        if group > 1 {
            // SAFETY: simple appel système ; un groupe disparu renvoie une erreur sans autre effet.
            unsafe {
                libc::killpg(group as i32, libc::SIGKILL);
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
        SetNamedSecurityInfoW, SetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SE_KERNEL_OBJECT,
    };
    use windows_sys::Win32::Security::{
        AclSizeInformation, EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorDacl, GetTokenInformation,
        IsWellKnownSid, TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACL,
        ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken, CREATE_SUSPENDED};

    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;

    #[link(name = "ntdll")]
    extern "system" {
        /// Relance un process créé suspendu. Présent dans toutes les versions de Windows NT.
        fn NtResumeProcess(process: HANDLE) -> i32;
    }

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn last() -> io::Error {
        io::Error::last_os_error()
    }

    /// Le compte sous lequel tourne le process, sous sa forme `S-1-5-…`, et le tampon qui
    /// porte son identifiant binaire.
    fn current_user() -> io::Result<(String, Vec<u64>)> {
        // SAFETY: chaque appel reçoit des tampons valides de la taille annoncée ; les poignées
        // et la mémoire rendues par le système sont libérées avant de sortir.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(last());
            }
            let mut buffer = vec![0u64; 64];
            let mut needed = 0u32;
            let read = GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), (buffer.len() * 8) as u32, &mut needed);
            let error = last();
            CloseHandle(token);
            if read == 0 {
                return Err(error);
            }
            let sid = (*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid;
            let mut text: *mut u16 = null_mut();
            if ConvertSidToStringSidW(sid, &mut text) == 0 {
                return Err(last());
            }
            let length = (0..).take_while(|&i| *text.add(i) != 0).count();
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
            Ok((name, buffer))
        }
    }

    fn user_sid(buffer: &[u64]) -> PSID {
        // SAFETY: le tampon a été rempli par GetTokenInformation(TokenUser) et vit plus longtemps que le pointeur.
        unsafe { (*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid }
    }

    /// Applique à un objet une liste de droits écrite en SDDL, sans héritage du dossier parent.
    fn with_dacl<T>(sddl: &str, apply: impl FnOnce(*const ACL) -> u32) -> io::Result<T>
    where
        T: Default,
    {
        // SAFETY: le descripteur rendu par le système est valide jusqu'à LocalFree, appelé après usage.
        unsafe {
            let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
            let text = wide(std::ffi::OsStr::new(sddl));
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), SDDL_REVISION_1, &mut descriptor, null_mut()) == 0 {
                return Err(last());
            }
            let (mut present, mut defaulted, mut dacl) = (0, 0, null_mut::<ACL>());
            let found = GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
            let code = if found == 0 || present == 0 || dacl.is_null() { 1 } else { apply(dacl) };
            LocalFree(descriptor.cast());
            if code != 0 {
                return Err(io::Error::from_raw_os_error(code as i32));
            }
            Ok(T::default())
        }
    }

    /// Le compte de l'agent, le système et les administrateurs (qui peuvent tout de toute
    /// façon) : personne d'autre, et rien d'hérité du dossier parent.
    pub fn make_private(path: &Path) -> io::Result<()> {
        let (user, _) = current_user()?;
        let inherit = if path.is_dir() { "OICI" } else { "" };
        let sddl = format!("D:P(A;{inherit};FA;;;{user})(A;{inherit};FA;;;SY)(A;{inherit};FA;;;BA)");
        let name = wide(path.as_os_str());
        with_dacl(&sddl, |dacl| {
            // SAFETY: `name` est une chaîne terminée par zéro, `dacl` est valide le temps de l'appel.
            unsafe {
                SetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    dacl,
                    null(),
                )
            }
        })
    }

    fn trusted(sid: PSID, me: PSID) -> bool {
        // SAFETY: les deux identifiants viennent du système et sont valides.
        unsafe { EqualSid(sid, me) != 0 || IsWellKnownSid(sid, WinLocalSystemSid) != 0 || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0 }
    }

    pub fn check_private(path: &Path) -> Result<(), String> {
        let shown = path.display();
        let (_, buffer) = current_user().map_err(|e| e.to_string())?;
        let me = user_sid(&buffer);
        let name = wide(path.as_os_str());
        // SAFETY: les pointeurs de sortie sont valides ; le descripteur rendu est libéré avant de sortir,
        // et rien de ce qu'il contient n'est gardé au-delà.
        unsafe {
            let (mut owner, mut dacl, mut descriptor): (PSID, *mut ACL, PSECURITY_DESCRIPTOR) = (null_mut(), null_mut(), null_mut());
            let code = GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            );
            if code != 0 {
                return Err(format!("{shown} : {}", io::Error::from_raw_os_error(code as i32)));
            }
            let verdict = (|| {
                if owner.is_null() || !trusted(owner, me) {
                    return Err(format!("{shown} appartient à un autre compte"));
                }
                if dacl.is_null() {
                    return Err(format!("{shown} est ouvert à tous les comptes (aucune liste de droits)"));
                }
                let mut size = ACL_SIZE_INFORMATION::default();
                if GetAclInformation(dacl, (&mut size as *mut ACL_SIZE_INFORMATION).cast(), size_of::<ACL_SIZE_INFORMATION>() as u32, AclSizeInformation) == 0 {
                    return Err(format!("{shown} : liste de droits illisible"));
                }
                for index in 0..size.AceCount {
                    let mut ace: *mut c_void = null_mut();
                    if GetAce(dacl, index, &mut ace) == 0 {
                        return Err(format!("{shown} : liste de droits illisible"));
                    }
                    let entry = ace as *const ACCESS_ALLOWED_ACE;
                    match (*entry).Header.AceType {
                        ACCESS_DENIED_ACE_TYPE => {}
                        ACCESS_ALLOWED_ACE_TYPE if trusted((&raw const (*entry).SidStart) as PSID, me) => {}
                        _ => return Err(format!("{shown} est accessible à d'autres comptes : retirez leurs droits (icacls /inheritance:r)")),
                    }
                }
                Ok(())
            })();
            LocalFree(descriptor.cast());
            verdict
        }
    }

    pub fn no_follow(options: &mut OpenOptions) {
        // un lien en bout de chemin est ouvert lui-même, pas sa cible : il est ensuite refusé
        // parce qu'il n'est pas un fichier ordinaire
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }

    pub fn identity(open: &File) -> io::Result<((u64, u64), u64)> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: la poignée vient d'un fichier ouvert, `info` est de la bonne taille.
        if unsafe { GetFileInformationByHandle(open.as_raw_handle() as HANDLE, &mut info) } == 0 {
            return Err(last());
        }
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Ok(((u64::from(info.dwVolumeSerialNumber), index), u64::from(info.nNumberOfLinks)))
    }

    pub fn identity_of(path: &Path) -> io::Result<((u64, u64), u64)> {
        // sans suivre de lien, et sans demander aucun droit sur le contenu
        let file = OpenOptions::new().access_mode(0).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).open(path)?;
        identity(&file)
    }

    fn new_job() -> io::Result<HANDLE> {
        // SAFETY: structures de la bonne taille, initialisées à zéro puis remplies ; la poignée
        // est fermée si le réglage échoue.
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return Err(last());
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            if SetInformationJobObject(job, JobObjectExtendedLimitInformation, (&raw const limits).cast(), size) == 0 {
                let error = last();
                CloseHandle(job);
                return Err(error);
            }
            Ok(job)
        }
    }

    pub fn harden_process() {
        // SAFETY: appels système sur le process courant ; la poignée du groupe est gardée
        // volontairement jusqu'à la fin du process : c'est sa fermeture, à la mort de l'agent,
        // qui fait tuer par le système tout ce qu'il a lancé.
        unsafe {
            SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
            match new_job() {
                Ok(job) if AssignProcessToJobObject(job, GetCurrentProcess()) != 0 => {}
                Ok(_) => eprintln!("groupe de process indisponible : {}", last()),
                Err(e) => eprintln!("groupe de process indisponible : {e}"),
            }
        }
        // les autres process du compte ne reçoivent plus que le droit d'attendre sa fin et de
        // le terminer : ni lecture de sa mémoire, ni injection
        let closed = current_user().and_then(|(user, _)| {
            let sddl = format!("D:P(A;;0x1FFFFF;;;SY)(A;;0x1FFFFF;;;BA)(A;;0x101001;;;{user})");
            with_dacl::<()>(&sddl, |dacl| {
                // SAFETY: poignée du process courant, `dacl` valide le temps de l'appel.
                unsafe {
                    SetSecurityInfo(
                        GetCurrentProcess(),
                        SE_KERNEL_OBJECT,
                        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                        null_mut(),
                        null_mut(),
                        dacl,
                        null(),
                    )
                }
            })
        });
        if let Err(e) = closed {
            eprintln!("fermeture du process aux autres process impossible : {e}");
        }
    }

    pub fn isolate(command: &mut Command) {
        // créée à l'arrêt : elle ne démarre qu'une fois rangée dans son groupe, sans quoi ce
        // qu'elle lancerait d'entrée lui échapperait
        command.creation_flags(CREATE_SUSPENDED);
    }

    pub fn adopt(child: &mut Child) -> Result<Group, String> {
        let Some(process) = child.raw_handle() else { return Err("process déjà terminé".into()) };
        let process = process as HANDLE;
        let fail = |child: &mut Child, error: io::Error| {
            let _ = child.start_kill();
            Err(format!("groupe de process impossible : {error}"))
        };
        let job = match new_job() {
            Ok(job) => job,
            Err(e) => return fail(child, e),
        };
        // SAFETY: `job` et `process` sont des poignées valides ; `job` est fermée en cas d'échec.
        unsafe {
            if AssignProcessToJobObject(job, process) == 0 {
                let error = last();
                CloseHandle(job);
                return fail(child, error);
            }
            if NtResumeProcess(process) < 0 {
                TerminateJobObject(job, 1);
                CloseHandle(job);
                return fail(child, io::Error::other("le process n'a pas pu démarrer"));
            }
        }
        Ok(job as Group)
    }

    pub fn kill_group(group: Group) {
        if group != 0 {
            // SAFETY: `group` est une poignée de groupe rendue par `adopt`, tuée et fermée une seule fois.
            unsafe {
                TerminateJobObject(group as HANDLE, 1);
                CloseHandle(group as HANDLE);
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
compile_error!("système non pris en charge : les protections de `os.rs` n'y sont pas écrites");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_fichier_secret_est_ferme_aux_autres_comptes() {
        let dir = tempfile::tempdir().unwrap();
        make_private(dir.path()).unwrap();
        let file = dir.path().join("cle");
        std::fs::write(&file, "secret").unwrap();
        make_private(&file).unwrap();
        check_private(&file).unwrap();
        check_private(dir.path()).unwrap();
        // ouvert aux autres : refusé
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(check_private(&file).is_err());
        }
        #[cfg(windows)]
        {
            let open = std::process::Command::new("icacls").arg(&file).args(["/grant", "*S-1-1-0:R"]).output().unwrap();
            assert!(open.status.success(), "icacls : {}", String::from_utf8_lossy(&open.stderr));
            assert!(check_private(&file).is_err(), "un fichier lisible par tous devait être refusé");
        }
    }

    #[test]
    fn un_fichier_ouvert_est_reconnu_et_un_lien_ne_trompe_pas() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let open = File::open(&a).unwrap();
        same_file(&open, &a).unwrap();
        assert!(same_file(&open, &b).is_err());
        assert_eq!(link_count(&open).unwrap(), 1);
        std::fs::hard_link(&a, dir.path().join("a2")).unwrap();
        assert_eq!(link_count(&open).unwrap(), 2);

        // un lien symbolique en bout de chemin n'est pas suivi
        let link = dir.path().join("lien");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&b, &link).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&b, &link).is_err() {
            return; // créer un lien demande un droit que ce compte n'a pas
        }
        let mut options = OpenOptions::new();
        options.read(true);
        no_follow(&mut options);
        let followed = options.open(&link).map(|f| f.metadata().unwrap().is_file()).unwrap_or(false);
        assert!(!followed, "le lien a été suivi");
    }

    #[tokio::test]
    async fn tout_ce_qu_une_commande_a_lance_part_avec_elle() {
        let dir = tempfile::tempdir().unwrap();
        let mark = dir.path().join("vivant");
        // la commande lance un petit-enfant qui écrirait un fichier après 3 s, puis rend la main
        #[cfg(unix)]
        let mut command = {
            let mut c = Command::new("/bin/sh");
            c.arg("-c").arg(format!("(sleep 3; echo x > '{}') & sleep 30", mark.display()));
            c
        };
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("cmd");
            c.arg("/c").arg(format!("start /b cmd /c \"ping -n 4 127.0.0.1 >nul & echo x > {}\" & ping -n 30 127.0.0.1 >nul", mark.display()));
            c
        };
        command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        isolate(&mut command);
        let mut child = command.spawn().unwrap();
        let group = adopt(&mut child).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        kill_group(group);
        let _ = child.wait().await;
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        assert!(!mark.exists(), "un process lancé par la commande lui a survécu");
    }
}
