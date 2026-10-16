# OpenCrosshair winget manifests, for submitting to microsoft/winget-pkgs under manifests/t/Tu2525/OpenCrosshair/<version>/.
New version: copy the latest version folder to a new folder named after the version.
Bump PackageVersion in all three files, the InstallerUrl tag and the ReleaseNotesUrl tag. Don't add DisplayVersion; the winget-pkgs review bot flags it when it equals PackageVersion.
Set InstallerSha256 to the SHA256 of the new OpenCrosshair-Setup.exe (Get-FileHash -Algorithm SHA256).
Run `winget validate` on the new folder before submitting it.
