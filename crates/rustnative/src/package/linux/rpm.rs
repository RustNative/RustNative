//! The RPM package format (v4, what `rpm` 4.x and 6.x read): what Fedora,
//! RHEL and its rebuilds, and openSUSE install with `dnf install ./app.rpm`
//! or `zypper install ./app.rpm`.
//!
//! A package is four parts back to back:
//!
//! 1. the **lead**, 96 bytes kept for compatibility (`rpm` reads only its
//!    magic and type);
//! 2. the **signature header** — here the package's sizes and the SHA-256
//!    of the main header (unsigned: a signature is added by `rpmsign`, which
//!    re-writes only this part), padded to eight bytes;
//! 3. the **main header** — the package's name, version, dependencies, and
//!    every file's path, mode, size, owner, and SHA-256 — with the payload's
//!    SHA-256;
//! 4. the **payload**, a `newc` cpio archive of the files, gzip'd.
//!
//! Both headers are the same structure (`rpm`'s `header.c`): a magic, the
//! number of index entries and data bytes, the index (tag, type, offset,
//! count — big-endian), and the data. Each starts with a *region* entry
//! whose data is a copy of an index entry at the very end, marking every
//! entry as part of the immutable, digested image.
//!
//! The gzip stream holds stored blocks (`super::gzip_stored`), so the
//! package is reproducible to the byte with no compressor to depend on.

use super::File;

// Header data types (`rpmtag.h`, `rpmTagType`).
const INT16: u32 = 3;
const INT32: u32 = 4;
const STRING: u32 = 6;
const BIN: u32 = 7;
const STRING_ARRAY: u32 = 8;
const I18NSTRING: u32 = 9;

// Signature header tags (`rpmtag.h`, `rpmSigTag`).
const HEADER_SIGNATURES: u32 = 62;
const SIG_SHA256: u32 = 273;
const SIG_SIZE: u32 = 1000;
const SIG_PAYLOAD_SIZE: u32 = 1007;

// Main header tags (`rpmtag.h`, `rpmTag`).
const HEADER_IMMUTABLE: u32 = 63;
const I18N_TABLE: u32 = 100;
const NAME: u32 = 1000;
const VERSION: u32 = 1001;
const RELEASE: u32 = 1002;
const SUMMARY: u32 = 1004;
const DESCRIPTION: u32 = 1005;
const BUILD_TIME: u32 = 1006;
const BUILD_HOST: u32 = 1007;
const SIZE: u32 = 1009;
const LICENSE: u32 = 1014;
const PACKAGER: u32 = 1015;
const GROUP: u32 = 1016;
const OS: u32 = 1021;
const ARCH: u32 = 1022;
const FILE_SIZES: u32 = 1028;
const FILE_MODES: u32 = 1030;
const FILE_RDEVS: u32 = 1033;
const FILE_MTIMES: u32 = 1034;
const FILE_DIGESTS: u32 = 1035;
const FILE_LINKTOS: u32 = 1036;
const FILE_FLAGS: u32 = 1037;
const FILE_USERNAME: u32 = 1039;
const FILE_GROUPNAME: u32 = 1040;
const SOURCE_RPM: u32 = 1044;
const FILE_VERIFY_FLAGS: u32 = 1045;
const PROVIDE_NAME: u32 = 1047;
const REQUIRE_FLAGS: u32 = 1048;
const REQUIRE_NAME: u32 = 1049;
const REQUIRE_VERSION: u32 = 1050;
const RPM_VERSION: u32 = 1064;
const FILE_DEVICES: u32 = 1095;
const FILE_INODES: u32 = 1096;
const FILE_LANGS: u32 = 1097;
const PROVIDE_FLAGS: u32 = 1112;
const PROVIDE_VERSION: u32 = 1113;
const DIR_INDEXES: u32 = 1116;
const BASENAMES: u32 = 1117;
const DIRNAMES: u32 = 1118;
const PAYLOAD_FORMAT: u32 = 1124;
const PAYLOAD_COMPRESSOR: u32 = 1125;
const PAYLOAD_FLAGS: u32 = 1126;
const FILE_DIGEST_ALGO: u32 = 5011;
const ENCODING: u32 = 5062;
const PAYLOAD_DIGEST: u32 = 5092;
const PAYLOAD_DIGEST_ALGO: u32 = 5093;
const PAYLOAD_DIGEST_ALT: u32 = 5097;

// Dependency flags (`rpmds.h`, `rpmsenseFlags`).
const SENSE_LESS: u32 = 1 << 1;
const SENSE_GREATER: u32 = 1 << 2;
const SENSE_EQUAL: u32 = 1 << 3;
const SENSE_RPMLIB: u32 = 1 << 24;

/// `PGPHASHALGO_SHA256`, the digest algorithm of the files and payload.
const SHA256: u32 = 8;

/// One header entry's value.
#[derive(Debug, Clone)]
enum Value {
    Int16(Vec<u16>),
    Int32(Vec<u32>),
    String(String),
    StringArray(Vec<String>),
    I18n(String),
}

impl Value {
    const fn kind(&self) -> u32 {
        match self {
            Self::Int16(_) => INT16,
            Self::Int32(_) => INT32,
            Self::String(_) => STRING,
            Self::StringArray(_) => STRING_ARRAY,
            Self::I18n(_) => I18NSTRING,
        }
    }

    fn count(&self) -> usize {
        match self {
            Self::Int16(values) => values.len(),
            Self::Int32(values) => values.len(),
            Self::String(_) | Self::I18n(_) => 1,
            Self::StringArray(values) => values.len(),
        }
    }

    /// The alignment the reader checks for this type.
    const fn alignment(&self) -> usize {
        match self {
            Self::Int16(_) => 2,
            Self::Int32(_) => 4,
            _ => 1,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::Int16(values) => values.iter().flat_map(|value| value.to_be_bytes()).collect(),
            Self::Int32(values) => values.iter().flat_map(|value| value.to_be_bytes()).collect(),
            Self::String(text) | Self::I18n(text) => [text.as_bytes(), &[0]].concat(),
            Self::StringArray(values) => {
                values.iter().flat_map(|text| [text.as_bytes(), &[0]].concat()).collect()
            }
        }
    }
}

fn strings<S: Into<String>>(values: impl IntoIterator<Item = S>) -> Value {
    Value::StringArray(values.into_iter().map(Into::into).collect())
}

/// Saturates at `u32::MAX`: RPM's 32-bit fields describe packages under
/// 4 GiB, which an application's executable and metadata are.
fn u32_of(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// A header (`header.c`'s on-disk form): magic, index, and data, with the
/// immutable `region` entry first and its trailer last.
fn header(region: u32, mut entries: Vec<(u32, Value)>) -> Vec<u8> {
    entries.sort_by_key(|(tag, _)| *tag);
    let count = entries.len() + 1;
    let mut index = Vec::with_capacity(count * 16);
    let mut data = Vec::new();
    let entry = |index: &mut Vec<u8>, tag: u32, kind: u32, offset: u32, count: u32| {
        for field in [tag, kind, offset, count] {
            index.extend_from_slice(&field.to_be_bytes());
        }
    };
    let mut rest = Vec::with_capacity(entries.len() * 16);
    for (tag, value) in &entries {
        data.resize(data.len().next_multiple_of(value.alignment()), 0);
        entry(&mut rest, *tag, value.kind(), u32_of(data.len()), u32_of(value.count()));
        data.extend_from_slice(&value.bytes());
    }
    // The region's own data is the trailer: an index entry naming the
    // region again, with the (negative) size of the index it covers.
    let trailer_offset = u32_of(data.len());
    let covered = i32::try_from(count * 16).unwrap_or(i32::MAX);
    entry(&mut data, region, BIN, u32::from_be_bytes((-covered).to_be_bytes()), 16);
    entry(&mut index, region, BIN, trailer_offset, 16);
    index.extend_from_slice(&rest);

    let mut out = vec![0x8e, 0xad, 0xe8, 0x01, 0, 0, 0, 0];
    out.extend_from_slice(&u32_of(count).to_be_bytes());
    out.extend_from_slice(&u32_of(data.len()).to_be_bytes());
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    out
}

/// A `newc` cpio archive of `files` (paths `./usr/...`), each with the
/// inode its position gives it, ended by the trailer entry.
fn cpio(files: &[File], mtime: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for (position, file) in files.iter().enumerate() {
        cpio_entry(&mut out, u32_of(position + 1), mode_of(file), mtime, &file.path, &file.data);
    }
    cpio_entry(&mut out, 0, 0, 0, "TRAILER!!!", &[]);
    out
}

/// One `newc` entry: the header's thirteen hexadecimal fields, the
/// NUL-terminated name, and the data, each padded to four bytes.
fn cpio_entry(out: &mut Vec<u8>, inode: u32, mode: u32, mtime: u32, name: &str, data: &[u8]) {
    let fields =
        [inode, mode, 0, 0, 1, mtime, u32_of(data.len()), 0, 0, 0, 0, u32_of(name.len() + 1), 0];
    out.extend_from_slice(b"070701");
    for field in fields {
        out.extend_from_slice(format!("{field:08x}").as_bytes());
    }
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    out.resize(out.len().next_multiple_of(4), 0);
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

const fn mode_of(file: &File) -> u32 {
    if file.executable { 0o100_755 } else { 0o100_644 }
}

/// What the package says about itself.
#[derive(Debug, Clone)]
pub struct Metadata<'a> {
    /// The package name (`[a-z0-9+._-]`).
    pub name: &'a str,
    /// The version (`major.minor.patch`).
    pub version: &'a str,
    /// The release of this version, `1` for the first package of it.
    pub release: &'a str,
    /// One line about it.
    pub summary: &'a str,
    /// Who packaged it.
    pub packager: &'a str,
    /// The architecture (`x86_64`, `aarch64`).
    pub arch: &'a str,
    /// What it requires: a name, and optionally `>=` that version.
    pub requires: &'a [(&'a str, Option<&'a str>)],
}

/// An RPM package of `files`, whose paths are absolute paths without the
/// leading `/` (`usr/bin/app`).
#[must_use]
pub fn rpm(metadata: &Metadata<'_>, files: &[File], time: u64) -> Vec<u8> {
    let mtime = u32::try_from(time).unwrap_or(u32::MAX);
    let mut files = files.to_vec();
    files.sort_by(|left, right| left.path.cmp(&right.path));

    // The payload names files `./usr/...`; the header splits each into the
    // directory (with its trailing `/`) and the base name.
    let in_payload: Vec<File> = files
        .iter()
        .map(|file| File { path: format!("./{}", file.path), ..file.clone() })
        .collect();
    let archive = cpio(&in_payload, mtime);
    let payload = super::gzip_stored(&archive);

    let mut dirnames: Vec<String> = Vec::new();
    let mut dir_indexes = Vec::new();
    let mut basenames = Vec::new();
    for file in &files {
        let (directory, base) = file.path.rsplit_once('/').unwrap_or(("", &file.path));
        let directory = format!("/{directory}/").replace("//", "/");
        let position = dirnames.iter().position(|known| *known == directory).unwrap_or_else(|| {
            dirnames.push(directory);
            dirnames.len() - 1
        });
        dir_indexes.push(u32_of(position));
        basenames.push(base.to_owned());
    }

    let full_version = format!("{}-{}", metadata.version, metadata.release);
    let mut require_names = Vec::new();
    let mut require_flags = Vec::new();
    let mut require_versions = Vec::new();
    for (name, version) in metadata.requires {
        require_names.push((*name).to_owned());
        require_flags.push(if version.is_some() { SENSE_GREATER | SENSE_EQUAL } else { 0 });
        require_versions.push(version.unwrap_or_default().to_owned());
    }
    // The format features this package uses, which `rpm` checks it has.
    for (feature, version) in [
        ("CompressedFileNames", "3.0.4-1"),
        ("FileDigests", "4.6.0-1"),
        ("PayloadFilesHavePrefix", "4.0-1"),
    ] {
        require_names.push(format!("rpmlib({feature})"));
        require_flags.push(SENSE_RPMLIB | SENSE_LESS | SENSE_EQUAL);
        require_versions.push(version.to_owned());
    }

    let count = files.len();
    let installed: usize = files.iter().map(|file| file.data.len()).sum();
    let main = header(
        HEADER_IMMUTABLE,
        vec![
            (I18N_TABLE, strings(["C"])),
            (NAME, Value::String(metadata.name.to_owned())),
            (VERSION, Value::String(metadata.version.to_owned())),
            (RELEASE, Value::String(metadata.release.to_owned())),
            (SUMMARY, Value::I18n(metadata.summary.to_owned())),
            (DESCRIPTION, Value::I18n(metadata.summary.to_owned())),
            (BUILD_TIME, Value::Int32(vec![mtime])),
            (BUILD_HOST, Value::String("localhost".to_owned())),
            (SIZE, Value::Int32(vec![u32_of(installed)])),
            (LICENSE, Value::String("Unspecified".to_owned())),
            (PACKAGER, Value::String(metadata.packager.to_owned())),
            (GROUP, Value::I18n("Unspecified".to_owned())),
            (OS, Value::String("linux".to_owned())),
            (ARCH, Value::String(metadata.arch.to_owned())),
            (FILE_SIZES, Value::Int32(files.iter().map(|file| u32_of(file.data.len())).collect())),
            (
                FILE_MODES,
                Value::Int16(
                    files
                        .iter()
                        .map(|file| u16::try_from(mode_of(file)).unwrap_or(0o644))
                        .collect(),
                ),
            ),
            (FILE_RDEVS, Value::Int16(vec![0; count])),
            (FILE_MTIMES, Value::Int32(vec![mtime; count])),
            (
                FILE_DIGESTS,
                strings(files.iter().map(|file| crate::package::zip::hash_of(&file.data))),
            ),
            (FILE_LINKTOS, strings(vec![""; count])),
            (FILE_FLAGS, Value::Int32(vec![0; count])),
            (FILE_USERNAME, strings(vec!["root"; count])),
            (FILE_GROUPNAME, strings(vec!["root"; count])),
            // Its presence is what makes `rpm` read this as a binary
            // package rather than a source one.
            (SOURCE_RPM, Value::String(format!("{}-{full_version}.src.rpm", metadata.name))),
            (FILE_VERIFY_FLAGS, Value::Int32(vec![u32::MAX; count])),
            (PROVIDE_NAME, strings([metadata.name])),
            (REQUIRE_FLAGS, Value::Int32(require_flags)),
            (REQUIRE_NAME, Value::StringArray(require_names)),
            (REQUIRE_VERSION, Value::StringArray(require_versions)),
            (RPM_VERSION, Value::String("4.18.0".to_owned())),
            (FILE_DEVICES, Value::Int32(vec![1; count])),
            (FILE_INODES, Value::Int32((1..=u32_of(count)).collect())),
            (FILE_LANGS, strings(vec![""; count])),
            (PROVIDE_FLAGS, Value::Int32(vec![SENSE_EQUAL])),
            (PROVIDE_VERSION, strings([full_version.as_str()])),
            (DIR_INDEXES, Value::Int32(dir_indexes)),
            (BASENAMES, Value::StringArray(basenames)),
            (DIRNAMES, Value::StringArray(dirnames)),
            (PAYLOAD_FORMAT, Value::String("cpio".to_owned())),
            (PAYLOAD_COMPRESSOR, Value::String("gzip".to_owned())),
            (PAYLOAD_FLAGS, Value::String("9".to_owned())),
            (FILE_DIGEST_ALGO, Value::Int32(vec![SHA256])),
            (ENCODING, Value::String("utf-8".to_owned())),
            (PAYLOAD_DIGEST, strings([crate::package::zip::hash_of(&payload)])),
            (PAYLOAD_DIGEST_ALGO, Value::Int32(vec![SHA256])),
            (PAYLOAD_DIGEST_ALT, strings([crate::package::zip::hash_of(&archive)])),
        ],
    );

    let mut signature = header(
        HEADER_SIGNATURES,
        vec![
            (SIG_SHA256, Value::String(crate::package::zip::hash_of(&main))),
            (SIG_SIZE, Value::Int32(vec![u32_of(main.len() + payload.len())])),
            (SIG_PAYLOAD_SIZE, Value::Int32(vec![u32_of(archive.len())])),
        ],
    );
    signature.resize(signature.len().next_multiple_of(8), 0);

    let mut out = lead(&format!("{}-{full_version}", metadata.name), metadata.arch);
    out.extend_from_slice(&signature);
    out.extend_from_slice(&main);
    out.extend_from_slice(&payload);
    out
}

/// The 96-byte lead: magic, format 3.0, binary, the architecture's old
/// number, the package's `name-version-release`, Linux, and "a header-style
/// signature follows".
fn lead(name: &str, arch: &str) -> Vec<u8> {
    let mut out = vec![0xed, 0xab, 0xee, 0xdb, 3, 0];
    out.extend_from_slice(&0_u16.to_be_bytes());
    let arch_number: u16 = match arch {
        "x86_64" | "i686" => 1,
        "aarch64" => 19,
        _ => 0,
    };
    out.extend_from_slice(&arch_number.to_be_bytes());
    let mut field = [0_u8; 66];
    let length = name.len().min(65);
    field[..length].copy_from_slice(&name.as_bytes()[..length]);
    out.extend_from_slice(&field);
    out.extend_from_slice(&1_u16.to_be_bytes());
    out.extend_from_slice(&5_u16.to_be_bytes());
    out.extend_from_slice(&[0; 16]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    /// The header at `at`: its index as (tag, type, offset, count), its
    /// data, and the offset just past it.
    fn parse(bytes: &[u8], at: usize) -> (Vec<[u32; 4]>, &[u8], usize) {
        assert_eq!(&bytes[at..at + 4], &[0x8e, 0xad, 0xe8, 0x01], "header magic");
        let count = read_u32(bytes, at + 8) as usize;
        let size = read_u32(bytes, at + 12) as usize;
        let index = (0..count)
            .map(|entry| {
                let base = at + 16 + entry * 16;
                [0, 4, 8, 12].map(|field| read_u32(bytes, base + field))
            })
            .collect();
        let start = at + 16 + count * 16;
        (index, &bytes[start..start + size], start + size)
    }

    fn string_at(data: &[u8], offset: u32) -> &str {
        let rest = &data[offset as usize..];
        std::str::from_utf8(&rest[..rest.iter().position(|byte| *byte == 0).expect("NUL")])
            .expect("UTF-8")
    }

    fn sample() -> Vec<u8> {
        rpm(
            &Metadata {
                name: "notes",
                version: "1.2.3",
                release: "1",
                summary: "Take notes",
                packager: "Example <example@example.com>",
                arch: "x86_64",
                requires: &[("gtk4", Some("4.14")), ("libsoup3", None)],
            },
            &[
                File {
                    path: "usr/bin/notes".to_owned(),
                    data: b"\x7fELF".to_vec(),
                    executable: true,
                },
                File {
                    path: "usr/share/applications/com.example.Notes.desktop".to_owned(),
                    data: b"[Desktop Entry]\n".to_vec(),
                    executable: false,
                },
            ],
            0,
        )
    }

    #[test]
    fn the_lead_and_both_headers_are_where_rpm_reads_them() {
        let package = sample();
        assert_eq!(&package[..4], &[0xed, 0xab, 0xee, 0xdb]);
        assert_eq!(&package[10..21], b"notes-1.2.3");
        let (signature, _, end) = parse(&package, 96);
        assert_eq!(signature[0][0], HEADER_SIGNATURES, "the region comes first");
        let main_start = end.next_multiple_of(8);
        let (main, data, main_end) = parse(&package, main_start);
        assert_eq!(main[0][0], HEADER_IMMUTABLE);
        // Index entries after the region are sorted by tag, and their data
        // in the same order, so no entry's data overlaps the next.
        let tags = main[1..].iter().map(|entry| entry[0]).collect::<Vec<_>>();
        assert!(tags.windows(2).all(|pair| pair[0] < pair[1]), "{tags:?}");
        let offsets = main[1..].iter().map(|entry| entry[2]).collect::<Vec<_>>();
        assert!(offsets.windows(2).all(|pair| pair[0] <= pair[1]));
        // The region's data is its trailer, the last 16 bytes, covering
        // every entry.
        assert_eq!(main[0][2] as usize, data.len() - 16);
        assert_eq!(read_u32(data, data.len() - 16), HEADER_IMMUTABLE);
        assert_eq!(
            i32::from_be_bytes(read_u32(data, data.len() - 8).to_be_bytes()),
            -i32::try_from(main.len() * 16).expect("small")
        );
        let name = main.iter().find(|entry| entry[0] == NAME).expect("a name");
        assert_eq!(string_at(data, name[2]), "notes");
        // Then the gzip'd payload.
        assert_eq!(&package[main_end..main_end + 2], &[0x1f, 0x8b]);
    }

    #[test]
    fn the_signature_header_digests_the_main_header_and_sizes_the_package() {
        let package = sample();
        let (signature, data, end) = parse(&package, 96);
        let main_start = end.next_multiple_of(8);
        let (_, _, main_end) = parse(&package, main_start);
        let digest = signature.iter().find(|entry| entry[0] == SIG_SHA256).expect("SHA-256");
        assert_eq!(
            string_at(data, digest[2]),
            crate::package::zip::hash_of(&package[main_start..main_end])
        );
        let size = signature.iter().find(|entry| entry[0] == SIG_SIZE).expect("size");
        assert_eq!(read_u32(data, size[2] as usize) as usize, package.len() - main_start);
    }

    #[test]
    fn integer_data_is_aligned_to_its_type() {
        let package = sample();
        let (_, _, end) = parse(&package, 96);
        let (main, _, _) = parse(&package, end.next_multiple_of(8));
        for [tag, kind, offset, _] in &main[1..] {
            let alignment = match *kind {
                INT16 => 2,
                INT32 => 4,
                _ => 1,
            };
            assert_eq!(offset % alignment, 0, "tag {tag}");
        }
    }

    #[test]
    fn the_cpio_payload_is_newc_with_a_trailer() {
        let archive = cpio(
            &[File { path: "./usr/bin/x".to_owned(), data: b"abc".to_vec(), executable: true }],
            0,
        );
        assert!(archive.starts_with(b"070701"));
        // The mode field (the second) says a regular, executable file.
        assert_eq!(&archive[14..22], b"000081ed");
        assert_eq!(archive.len() % 4, 0);
        let text = String::from_utf8_lossy(&archive);
        assert!(text.contains("./usr/bin/x\0") && text.contains("TRAILER!!!\0"));
    }

    #[test]
    fn the_same_inputs_give_the_same_bytes() {
        assert_eq!(sample(), sample());
    }

    /// Where `rpm` is installed (Fedora, openSUSE; Debian has it too), it
    /// reads the package back: its metadata, its file list, and its digests.
    #[test]
    fn rpm_itself_reads_the_package_and_verifies_its_digests() {
        let Some(tool) = super::super::which("rpm") else {
            eprintln!("skipped: rpm is not installed");
            return;
        };
        let directory = std::env::temp_dir().join(format!("rustnative-rpm-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        let path = directory.join("notes-1.2.3-1.x86_64.rpm");
        std::fs::write(&path, sample()).expect("written");
        let run = |args: &[&str]| {
            let output =
                std::process::Command::new(&tool).args(args).arg(&path).output().expect("rpm runs");
            let text = String::from_utf8_lossy(&output.stdout).into_owned()
                + &String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "rpm {args:?}: {text}");
            text
        };
        let query = run(&[
            "-qp",
            "--queryformat",
            "%{NAME} %{VERSION} %{RELEASE} %{ARCH} %{SUMMARY}\n[%{REQUIRENEVRS}\n]",
        ]);
        assert!(query.starts_with("notes 1.2.3 1 x86_64 Take notes\n"), "{query}");
        assert!(query.contains("gtk4 >= 4.14") && query.contains("libsoup3"), "{query}");
        let list = run(&["-qlp"]);
        assert_eq!(
            list.lines().collect::<Vec<_>>(),
            ["/usr/bin/notes", "/usr/share/applications/com.example.Notes.desktop"]
        );
        let check = run(&["--checksig", "--nosignature"]);
        assert!(check.contains("digests OK"), "{check}");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
