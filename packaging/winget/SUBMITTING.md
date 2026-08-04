# Getting AudioRemote into winget

Only the **first** version is submitted by hand. After that the `winget-submit`
job in `.github/workflows/release.yml` opens the pull request at every release,
because `wingetcreate update` edits manifests that already exist upstream — it
cannot create a package that has never been there.

## What is in this folder

Three files, which is what winget's schema 1.6 requires for one version:

| File | Holds |
|---|---|
| `ishizakahiroshi.AudioRemote.yaml` | the version and which locale is the default |
| `ishizakahiroshi.AudioRemote.installer.yaml` | the download URL, its SHA256, and how to unpack it |
| `ishizakahiroshi.AudioRemote.locale.en-US.yaml` | everything a person reads: name, description, licence, links |

They are the master copy. The ones users install from live in
`microsoft/winget-pkgs`; these stay here so the package can be rebuilt from the
repository that produces it.

## Before submitting

```powershell
winget validate --manifest packaging\winget
```

To install from the local files as a rehearsal, winget needs a setting turned on
first, and that needs an elevated prompt:

```powershell
winget settings --enable LocalManifestFiles   # run as administrator, once
winget install --manifest packaging\winget
```

This is a machine-wide switch that lets winget install from any local manifest,
so leave it off unless you are actually testing packages. Without it, the check
that matters can still be done directly:

```powershell
# the URL resolves, and its hash is the one the manifest claims
$url = 'https://github.com/ishizakahiroshi/audioremote/releases/download/v0.1.0/audioremote-win32-x64.zip'
Invoke-WebRequest $url -OutFile $env:TEMP\ar.zip
(Get-FileHash $env:TEMP\ar.zip -Algorithm SHA256).Hash
```

## The first submission

1. Fork `microsoft/winget-pkgs`.
2. Copy the three files to
   `manifests/i/ishizakahiroshi/AudioRemote/<version>/` in the fork — the path
   is derived from the identifier and is not optional.
3. Open a pull request against `microsoft/winget-pkgs`.
4. Their bots validate the manifest, install the package in a sandbox and run
   the smoke test. Expect a day or two.

## After it is merged

Add a repository secret so the release workflow can take over:

- **Name**: `WINGET_PKGS_TOKEN`
- **Value**: a GitHub personal access token with `public_repo` scope, on the
  account that owns the winget-pkgs fork

The workflow skips the winget channel entirely when the secret is absent, so
releases keep working until you add it.

## Things that will bite

- **The identifier is permanent.** `ishizakahiroshi.AudioRemote` cannot be
  renamed later without publishing a new package and orphaning the old one.
- **Prereleases are excluded on purpose.** `winget install` with no version
  argument takes the newest manifest, so an `-rc` published here would be handed
  to everyone who typed the plain command. The workflow skips any tag containing
  a hyphen.
- **The hash is copied from the release's `SHA256SUMS.txt`, never recomputed.**
  Two places claiming to know the digest is two places that can disagree.
- **The exe is unsigned.** winget accepts it, but SmartScreen still warns on
  first run. That is a code-signing question, not a packaging one.
