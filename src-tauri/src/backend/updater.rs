use std::{
    fs,
    io,
    path::{Path, PathBuf},
    process::Command,
    sync::RwLock,
    time::{SystemTime, UNIX_EPOCH},
};

use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT},
};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use super::models::{UpdateChannelId, UpdateStatus};

type DynError = Box<dyn std::error::Error + Send + Sync>;

const UPDATE_REPO_SLUG: &str = "noahcabral/aeroforge-nitrosense-alternative";
const UPDATE_STATE_FILE: &str = "update-state.json";
const UPDATES_DIR_NAME: &str = "updates";
const PORTABLE_ASSET_PREFIX: &str = "AeroForge-Control-Portable-";

pub struct UpdaterStore {
    status: RwLock<UpdateStatus>,
    status_file: PathBuf,
    updates_dir: PathBuf,
}

impl UpdaterStore {
    pub fn load(config_root: &Path) -> Result<Self, DynError> {
        let status_file = config_root.join(UPDATE_STATE_FILE);
        let updates_dir = config_root.join(UPDATES_DIR_NAME);
        fs::create_dir_all(&updates_dir)?;

        let status = load_status(&status_file)?.unwrap_or_else(default_status);

        Ok(Self {
            status: RwLock::new(apply_runtime_fields(status)),
            status_file,
            updates_dir,
        })
    }

    pub fn status(&self) -> UpdateStatus {
        let current = self
            .status
            .read()
            .expect("updater status lock poisoned")
            .clone();
        apply_runtime_fields(current)
    }

    pub fn save_status(&self, status: UpdateStatus) -> Result<UpdateStatus, DynError> {
        let runtime_status = apply_runtime_fields(status);
        fs::write(&self.status_file, serde_json::to_string_pretty(&runtime_status)?)?;

        {
            let mut guard = self.status.write().expect("updater status lock poisoned");
            *guard = runtime_status.clone();
        }

        Ok(runtime_status)
    }

    pub fn updates_dir(&self) -> &Path {
        &self.updates_dir
    }
}

pub fn refresh_status(store: &UpdaterStore, channel: UpdateChannelId) -> Result<UpdateStatus, DynError> {
    let resolved = resolve_update_candidate(channel, store.status())?;
    store.save_status(resolved.status)
}

pub fn stage_latest_update(
    store: &UpdaterStore,
    channel: UpdateChannelId,
) -> Result<UpdateStatus, DynError> {
    let resolved = resolve_update_candidate(channel, store.status())?;
    let candidate = resolved
        .asset
        .ok_or_else(|| io::Error::other("No portable update asset is available for the selected channel."))?;

    let stage_root = store
        .updates_dir()
        .join(resolved
            .status
            .latest_version
            .clone()
            .unwrap_or_else(|| "unknown".into())
            .replace(['/', '\\', ':'], "-"));
    fs::create_dir_all(&stage_root)?;
    let staged_file = stage_root.join(&candidate.name);

    let mut response = github_client()?
        .get(&candidate.browser_download_url)
        .send()?
        .error_for_status()?;
    let mut file = fs::File::create(&staged_file)?;
    io::copy(&mut response, &mut file)?;

    let staged_bytes = fs::read(&staged_file)?;
    let staged_sha256 = format!("{:x}", Sha256::digest(&staged_bytes));

    let mut status = resolved.status;
    status.staged_asset_name = Some(candidate.name.clone());
    status.staged_asset_path = Some(staged_file.display().to_string());
    status.staged_sha256 = Some(staged_sha256);
    status.staged_at_unix = Some(now_unix());
    status.can_install_update = true;
    status.detail = format!(
        "Downloaded {} and staged it for install.",
        candidate.name
    );
    status.last_error = None;

    store.save_status(status)
}

pub fn launch_staged_install(store: &UpdaterStore) -> Result<UpdateStatus, DynError> {
    let mut status = store.status();
    let staged_path = status
        .staged_asset_path
        .clone()
        .ok_or_else(|| io::Error::other("No staged update file is available yet."))?;
    let staged_zip = PathBuf::from(&staged_path);
    if !staged_zip.exists() {
        return Err(io::Error::other("The staged update file no longer exists on disk.").into());
    }
    if staged_zip
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| !extension.eq_ignore_ascii_case("zip"))
        .unwrap_or(true)
    {
        return Err(io::Error::other(
            "Only portable ZIP update assets can be installed automatically right now.",
        )
        .into());
    }

    let current_exe = std::env::current_exe()?;
    let target_dir = current_exe
        .parent()
        .ok_or_else(|| io::Error::other("Current executable path is missing a parent directory."))?
        .to_path_buf();
    ensure_writable_directory(&target_dir)?;

    let extract_root = store.updates_dir().join("install").join(now_unix().to_string());
    if extract_root.exists() {
        fs::remove_dir_all(&extract_root)?;
    }
    fs::create_dir_all(&extract_root)?;
    extract_zip(&staged_zip, &extract_root)?;

    let staged_exe = extract_root.join(
        current_exe
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("Could not resolve the current executable name."))?,
    );
    if !staged_exe.exists() {
        return Err(io::Error::other(
            "The staged ZIP did not contain a runnable AeroForge executable.",
        )
        .into());
    }

    let script_path = store.updates_dir().join("apply-staged-update.ps1");
    fs::write(&script_path, install_script_body())?;

    let current_pid = std::process::id().to_string();
    let exe_name = current_exe
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::other("Could not resolve the current executable file name."))?
        .to_string();

    Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-WindowStyle")
        .arg("Hidden")
        .arg("-File")
        .arg(&script_path)
        .arg("-WaitPid")
        .arg(&current_pid)
        .arg("-SourceDir")
        .arg(&extract_root)
        .arg("-TargetDir")
        .arg(&target_dir)
        .arg("-ExeName")
        .arg(&exe_name)
        .spawn()?;

    status.can_install_update = true;
    status.detail =
        "Staged installer launched. Close AeroForge now so the updater can replace the portable files.".into();
    status.last_error = None;
    store.save_status(status)
}

fn resolve_update_candidate(
    channel: UpdateChannelId,
    existing: UpdateStatus,
) -> Result<ResolvedUpdateCandidate, DynError> {
    let client = github_client()?;
    let repo = fetch_repo(&client)?;
    let releases = fetch_releases(&client)?;

    let mut status = existing;
    status.repo_slug = UPDATE_REPO_SLUG.into();
    status.last_checked_at_unix = Some(now_unix());
    status.last_error = None;
    status.token_configured = false;

    if let Some(release) = select_release(&releases, &channel) {
        let latest_version = normalize_release_version(&release.tag_name)
            .or_else(|| release.name.clone())
            .unwrap_or_else(|| release.tag_name.clone());
        let latest_asset = select_portable_asset(&release.assets);
        let current_version = Version::parse(env!("CARGO_PKG_VERSION")).ok();
        let release_version = normalize_release_version(&release.tag_name)
            .and_then(|value| Version::parse(&value).ok());

        status.feed_kind = "release".into();
        status.latest_version = Some(latest_version.clone());
        status.latest_title = release
            .name
            .clone()
            .or_else(|| Some(release.tag_name.clone()));
        status.latest_published_at = release.published_at.clone();
        status.latest_commit_sha = None;
        status.latest_asset_name = latest_asset.as_ref().map(|asset| asset.name.clone());
        status.update_available = match (current_version, release_version) {
            (Some(current), Some(remote)) => remote > current,
            _ => latest_version != env!("CARGO_PKG_VERSION"),
        };
        status.can_stage_update = latest_asset.is_some() && status.update_available;
        status.can_install_update = staged_file_exists(&status);
        status.detail = if let Some(asset) = latest_asset.as_ref() {
            if status.update_available {
                format!(
                    "{} is available from the {} channel. Portable asset {} is ready to stage.",
                    latest_version,
                    channel.as_str(),
                    asset.name
                )
            } else {
                format!(
                    "AeroForge is already on {}. Latest published portable asset is {}.",
                    latest_version, asset.name
                )
            }
        } else {
            format!(
                "{} is published on the {} channel, but no portable ZIP asset is attached yet.",
                latest_version,
                channel.as_str()
            )
        };

        return Ok(ResolvedUpdateCandidate {
            status,
            asset: latest_asset,
        });
    }

    if matches!(channel, UpdateChannelId::Preview) {
        if repo.size == 0 {
            status.feed_kind = "none".into();
            status.latest_version = None;
            status.latest_title = Some("Repository empty".into());
            status.latest_published_at = None;
            status.latest_commit_sha = None;
            status.latest_asset_name = None;
            status.update_available = false;
            status.can_stage_update = false;
            status.can_install_update = staged_file_exists(&status);
            status.detail =
                "Preview channel is configured, but the selected repo has no commits or releases yet.".into();

            return Ok(ResolvedUpdateCandidate { status, asset: None });
        }

        let commit = match fetch_commit(&client, &repo.default_branch) {
            Ok(commit) => commit,
            Err(error) => {
                status.feed_kind = "none".into();
                status.latest_version = None;
                status.latest_title = Some(format!("{} unresolved", repo.default_branch));
                status.latest_published_at = None;
                status.latest_commit_sha = None;
                status.latest_asset_name = None;
                status.update_available = false;
                status.can_stage_update = false;
                status.can_install_update = staged_file_exists(&status);
                status.last_error = Some(error.to_string());
                status.detail = format!(
                    "Preview channel could not resolve {} yet. {}",
                    repo.default_branch,
                    error
                );

                return Ok(ResolvedUpdateCandidate { status, asset: None });
            }
        };
        status.feed_kind = "commit".into();
        status.latest_version = None;
        status.latest_title = Some(format!("{} branch head", repo.default_branch));
        status.latest_published_at = Some(commit.commit.committer.date.clone());
        status.latest_commit_sha = Some(short_sha(&commit.sha));
        status.latest_asset_name = None;
        status.update_available = false;
        status.can_stage_update = false;
        status.can_install_update = staged_file_exists(&status);
        status.detail = format!(
            "Preview is tracking {} at {}, but there is no published preview release asset to stage yet.",
            repo.default_branch,
            short_sha(&commit.sha)
        );

        return Ok(ResolvedUpdateCandidate { status, asset: None });
    }

    status.feed_kind = "none".into();
    status.latest_version = None;
    status.latest_title = None;
    status.latest_published_at = None;
    status.latest_commit_sha = None;
    status.latest_asset_name = None;
    status.update_available = false;
    status.can_stage_update = false;
    status.can_install_update = staged_file_exists(&status);
    status.detail = "Stable channel has no published release yet.".into();

    Ok(ResolvedUpdateCandidate { status, asset: None })
}

fn github_client() -> Result<Client, DynError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("AeroForgeControlUpdater/0.1.0"),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );

    Ok(Client::builder().default_headers(headers).build()?)
}

fn fetch_repo(client: &Client) -> Result<GithubRepo, DynError> {
    Ok(client
        .get(format!("https://api.github.com/repos/{UPDATE_REPO_SLUG}"))
        .send()?
        .error_for_status()?
        .json()?)
}

fn fetch_releases(client: &Client) -> Result<Vec<GithubRelease>, DynError> {
    Ok(client
        .get(format!(
            "https://api.github.com/repos/{UPDATE_REPO_SLUG}/releases?per_page=10"
        ))
        .send()?
        .error_for_status()?
        .json()?)
}

fn fetch_commit(client: &Client, branch: &str) -> Result<GithubCommitEnvelope, DynError> {
    Ok(client
        .get(format!(
            "https://api.github.com/repos/{UPDATE_REPO_SLUG}/commits/{branch}"
        ))
        .send()?
        .error_for_status()?
        .json()?)
}

fn select_release(releases: &[GithubRelease], channel: &UpdateChannelId) -> Option<GithubRelease> {
    match channel {
        UpdateChannelId::Stable => releases
            .iter()
            .find(|release| !release.draft && !release.prerelease)
            .cloned(),
        UpdateChannelId::Preview => releases.iter().find(|release| !release.draft).cloned(),
    }
}

fn select_portable_asset(assets: &[GithubAsset]) -> Option<GithubAsset> {
    assets
        .iter()
        .find(|asset| {
            asset.name.starts_with(PORTABLE_ASSET_PREFIX)
                && asset.name.to_ascii_lowercase().ends_with(".zip")
        })
        .cloned()
        .or_else(|| {
            assets
                .iter()
                .find(|asset| asset.name.to_ascii_lowercase().ends_with(".zip"))
                .cloned()
        })
}

fn normalize_release_version(tag_name: &str) -> Option<String> {
    let trimmed = tag_name.trim().trim_start_matches(['v', 'V']);
    Version::parse(trimmed).ok().map(|version| version.to_string())
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn default_status() -> UpdateStatus {
    UpdateStatus {
        repo_slug: UPDATE_REPO_SLUG.into(),
        current_version: env!("CARGO_PKG_VERSION").into(),
        token_configured: false,
        last_checked_at_unix: None,
        update_available: false,
        can_stage_update: false,
        can_install_update: false,
        feed_kind: "none".into(),
        latest_version: None,
        latest_title: None,
        latest_published_at: None,
        latest_commit_sha: None,
        latest_asset_name: None,
        staged_asset_name: None,
        staged_asset_path: None,
        staged_sha256: None,
        staged_at_unix: None,
        detail: "Updater not checked yet.".into(),
        last_error: None,
    }
}

fn apply_runtime_fields(mut status: UpdateStatus) -> UpdateStatus {
    status.repo_slug = UPDATE_REPO_SLUG.into();
    status.current_version = env!("CARGO_PKG_VERSION").into();
    status.token_configured = false;
    status.can_install_update = staged_file_exists(&status);
    status
}

fn load_status(path: &Path) -> Result<Option<UpdateStatus>, DynError> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        quarantine_invalid_state_file(path, "empty")?;
        return Ok(None);
    }

    match serde_json::from_str::<UpdateStatus>(&raw) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(_) => {
            quarantine_invalid_state_file(path, "invalid")?;
            Ok(None)
        }
    }
}

fn staged_file_exists(status: &UpdateStatus) -> bool {
    status
        .staged_asset_path
        .as_ref()
        .map(PathBuf::from)
        .map(|path| path.exists())
        .unwrap_or(false)
}

fn quarantine_invalid_state_file(path: &Path, reason: &str) -> Result<(), io::Error> {
    if !path.exists() {
        return Ok(());
    }

    let stamp = now_unix();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("update-state.json");
    let backup_name = format!("{file_name}.{reason}.{stamp}.bak");
    let backup_path = path.with_file_name(backup_name);
    fs::rename(path, backup_path)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn extract_zip(zip_path: &Path, destination: &Path) -> Result<(), DynError> {
    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| io::Error::other("The update ZIP contained an unsafe path."))?;
        let output_path = destination.join(enclosed);

        if entry.name().ends_with('/') {
            fs::create_dir_all(&output_path)?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut output = fs::File::create(&output_path)?;
        io::copy(&mut entry, &mut output)?;
    }

    Ok(())
}

fn ensure_writable_directory(path: &Path) -> Result<(), DynError> {
    let probe = path.join(".aeroforge-update-probe");
    fs::write(&probe, [])?;
    fs::remove_file(probe)?;
    Ok(())
}

fn install_script_body() -> &'static str {
    r#"
param(
  [Parameter(Mandatory = $true)][int]$WaitPid,
  [Parameter(Mandatory = $true)][string]$SourceDir,
  [Parameter(Mandatory = $true)][string]$TargetDir,
  [Parameter(Mandatory = $true)][string]$ExeName
)

$ErrorActionPreference = 'Stop'
$deadline = (Get-Date).AddMinutes(2)

while (Get-Process -Id $WaitPid -ErrorAction SilentlyContinue) {
  if ((Get-Date) -gt $deadline) {
    throw 'Timed out waiting for AeroForge to exit before update.'
  }
  Start-Sleep -Milliseconds 500
}

Copy-Item -Path (Join-Path $SourceDir '*') -Destination $TargetDir -Recurse -Force
Start-Sleep -Milliseconds 250
Start-Process -FilePath (Join-Path $TargetDir $ExeName)
"#
}

#[derive(Debug, Deserialize)]
struct GithubRepo {
    default_branch: String,
    size: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    prerelease: bool,
    draft: bool,
    published_at: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubCommitEnvelope {
    sha: String,
    commit: GithubCommitDetail,
}

#[derive(Debug, Deserialize)]
struct GithubCommitDetail {
    committer: GithubCommitter,
}

#[derive(Debug, Deserialize)]
struct GithubCommitter {
    date: String,
}

struct ResolvedUpdateCandidate {
    status: UpdateStatus,
    asset: Option<GithubAsset>,
}
