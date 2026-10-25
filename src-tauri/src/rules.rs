//! Эвристики: почему процесс выглядит подозрительно.
//! Чистые функции без обращений к системе, чтобы их было легко тестировать.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Reason {
    pub pts: u32,
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Verdict {
    pub score: u32,
    pub reasons: Vec<Reason>,
}

impl Verdict {
    fn add(&mut self, pts: u32, title: &str, text: String) {
        self.score = (self.score + pts).min(100);
        self.reasons.push(Reason { pts, title: title.into(), text });
    }
}

/// Что известно о процессе на момент проверки.
pub struct ProcFacts<'a> {
    pub name: &'a str,
    /// Полный путь к exe, если удалось прочитать (у защищённых процессов его нет).
    pub path: Option<&'a str>,
    pub parent_name: Option<&'a str>,
    /// `Some(false)` — подписи нет или она недействительна; `None` — не проверяли.
    pub signed: Option<bool>,
}

/// Системные процессы Windows и папки, из которых они обязаны запускаться.
const SYSTEM_NAMES: &[(&str, &[&str])] = &[
    ("svchost.exe", &[r"c:\windows\system32\", r"c:\windows\syswow64\"]),
    ("lsass.exe", &[r"c:\windows\system32\"]),
    ("csrss.exe", &[r"c:\windows\system32\"]),
    ("smss.exe", &[r"c:\windows\system32\"]),
    ("wininit.exe", &[r"c:\windows\system32\"]),
    ("winlogon.exe", &[r"c:\windows\system32\"]),
    ("services.exe", &[r"c:\windows\system32\"]),
    ("spoolsv.exe", &[r"c:\windows\system32\"]),
    ("taskhostw.exe", &[r"c:\windows\system32\"]),
    ("dllhost.exe", &[r"c:\windows\system32\", r"c:\windows\syswow64\"]),
    ("conhost.exe", &[r"c:\windows\system32\"]),
    ("rundll32.exe", &[r"c:\windows\system32\", r"c:\windows\syswow64\"]),
    ("explorer.exe", &[r"c:\windows\"]),
];

/// Папки, откуда нормальные программы обычно не запускаются.
const RISKY_DIRS: &[(&str, &str)] = &[
    (r"\appdata\local\temp\", "временной папки"),
    (r"\windows\temp\", "временной папки Windows"),
    (r"\downloads\", "папки «Загрузки»"),
    (r"\$recycle.bin\", "корзины"),
    (r"\users\public\", "общей папки пользователей"),
];

/// Программы, которые не должны запускать командные оболочки.
const DOC_APPS: &[&str] = &[
    "winword.exe", "excel.exe", "powerpnt.exe", "outlook.exe", "onenote.exe",
    "acrord32.exe", "acrobat.exe", "foxitpdfreader.exe",
];

const SHELLS: &[&str] = &[
    "powershell.exe", "pwsh.exe", "cmd.exe", "wscript.exe", "cscript.exe",
    "mshta.exe", "rundll32.exe", "regsvr32.exe",
];

pub fn analyze(p: &ProcFacts) -> Verdict {
    let mut v = Verdict::default();
    let name = p.name.to_lowercase();
    let path = p.path.map(|s| s.to_lowercase());

    // 1. Системное имя, но не из своей папки.
    if let (Some((_, dirs)), Some(path)) = (SYSTEM_NAMES.iter().find(|(n, _)| *n == name), &path) {
        let home_ok = dirs.iter().any(|d| {
            path.starts_with(d) && !path[d.len()..].contains('\\')
        });
        if !home_ok {
            v.add(40, "Маскируется под системный процесс", format!(
                "Настоящий {} запускается только из {}. Этот запущен из {}.",
                p.name, dirs[0].trim_end_matches('\\'), parent_dir(p.path.unwrap_or_default())
            ));
        }
    }

    // 2. Имя-двойник: на одну букву отличается от системного.
    if !SYSTEM_NAMES.iter().any(|(n, _)| *n == name) {
        if let Some((twin, _)) = SYSTEM_NAMES.iter().find(|(n, _)| is_lookalike(&name, n)) {
            v.add(35, "Имя похоже на системное", format!(
                "{} почти совпадает с {}. Так вирусы прячутся среди процессов Windows.",
                p.name, twin
            ));
        }
    }

    // 3. Запуск из «плохой» папки.
    if let Some(path) = &path {
        if let Some((_, label)) = RISKY_DIRS.iter().find(|(d, _)| path.contains(d)) {
            v.add(20, "Запущен из необычного места", format!(
                "Программа работает прямо из {}. Установленные программы так обычно не делают.", label
            ));
        }
    }

    // 4. Нет цифровой подписи. Само по себе это нормально для многих честных
    // программ, поэтому только усиливает уже найденные подозрения.
    if p.signed == Some(false) && v.score > 0 {
        v.add(15, "Нет цифровой подписи", "Нельзя проверить, кто выпустил эту программу.".into());
    }

    // 5. Документ запускает командную оболочку.
    if let Some(parent) = p.parent_name.map(|s| s.to_lowercase()) {
        if DOC_APPS.contains(&parent.as_str()) && SHELLS.contains(&name.as_str()) {
            v.add(30, "Странный запуск", format!(
                "{} запустил {}. Так часто срабатывают вредоносные макросы в документах.",
                p.parent_name.unwrap_or_default(), p.name
            ));
        }
    }

    v
}

/// Уровень для интерфейса: danger / warn / calm.
pub fn level(score: u32) -> &'static str {
    match score {
        60.. => "danger",
        30.. => "warn",
        _ => "calm",
    }
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('\\').map(|(d, _)| d).unwrap_or(path)
}

/// Расстояние Левенштейна ровно 1 (замена, вставка или удаление одной буквы)
/// либо перестановка двух соседних букв.
fn is_lookalike(a: &str, b: &str) -> bool {
    if a == b {
        return false;
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len() == b.len() {
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        return diff.len() == 1
            || (diff.len() == 2 && diff[1] == diff[0] + 1 && a[diff[0]] == b[diff[1]] && a[diff[1]] == b[diff[0]]);
    }
    let (short, long) = if a.len() < b.len() { (&a, &b) } else { (&b, &a) };
    if long.len() - short.len() != 1 {
        return false;
    }
    let i = (0..short.len()).find(|&i| short[i] != long[i]).unwrap_or(short.len());
    short[i..] == long[i + 1..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(name: &'a str, path: &'a str) -> ProcFacts<'a> {
        ProcFacts { name, path: Some(path), parent_name: None, signed: None }
    }

    #[test]
    fn real_svchost_is_clean() {
        let v = analyze(&facts("svchost.exe", r"C:\Windows\System32\svchost.exe"));
        assert_eq!(v.score, 0);
    }

    #[test]
    fn svchost_from_temp_is_danger() {
        let v = analyze(&ProcFacts {
            signed: Some(false),
            ..facts("svchost.exe", r"C:\Users\andre\AppData\Local\Temp\svchost.exe")
        });
        assert_eq!(v.score, 75);
        assert_eq!(level(v.score), "danger");
    }

    #[test]
    fn svchost_in_system32_subfolder_is_flagged() {
        let v = analyze(&facts("svchost.exe", r"C:\Windows\System32\evil\svchost.exe"));
        assert_eq!(v.score, 40);
    }

    #[test]
    fn lookalike_names() {
        assert!(is_lookalike("scvhost.exe", "svchost.exe"));
        assert!(is_lookalike("svchost1.exe", "svchost.exe"));
        assert!(is_lookalike("lsas.exe", "lsass.exe"));
        assert!(!is_lookalike("chrome.exe", "svchost.exe"));
        let v = analyze(&facts("expl0rer.exe", r"C:\Games\expl0rer.exe"));
        assert_eq!(v.score, 35);
    }

    #[test]
    fn word_spawning_powershell() {
        let v = analyze(&ProcFacts {
            name: "powershell.exe",
            path: Some(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            parent_name: Some("WINWORD.EXE"),
            signed: None,
        });
        assert_eq!(v.score, 30);
        assert_eq!(level(v.score), "warn");
    }

    #[test]
    fn unsigned_alone_is_not_suspicious() {
        let v = analyze(&ProcFacts {
            signed: Some(false),
            ..facts("python.exe", r"C:\Users\andre\AppData\Local\Programs\Python\python.exe")
        });
        assert_eq!(v.score, 0);
    }

    #[test]
    fn normal_program_is_clean() {
        let v = analyze(&ProcFacts {
            signed: Some(true),
            ..facts("obs64.exe", r"C:\Program Files\obs-studio\bin\64bit\obs64.exe")
        });
        assert_eq!(v.score, 0);
    }
}
