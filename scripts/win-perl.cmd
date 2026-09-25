@echo off
rem ─────────────────────────────────────────────────────────────────────────────
rem Full-Perl resolver for the bundled-OpenSSL Windows build.
rem
rem The Windows build compiles OpenSSL from source (see the `ssh2` dependency
rem comment in src-tauri/Cargo.toml), and that build is driven by OpenSSL's
rem `Configure`, a Perl program requiring the full core module set.
rem
rem Git for Windows ships a *trimmed* Perl. It is missing, among others,
rem `Locale::Maketext::Simple` — pulled in by `Params::Check` and `IPC::Cmd` —
rem and because Git Bash places its own /usr/bin ahead of every Windows PATH
rem entry, that trimmed Perl is the one a bare `perl` resolves to inside Git
rem Bash. `Configure` then dies before generating a single makefile:
rem
rem     Can't locate Locale/Maketext/Simple.pm in @INC
rem     Error configuring OpenSSL build: 'perl' reported failure with exit code: 2
rem
rem `src-tauri/.cargo/config.toml` points OPENSSL_SRC_PERL at this file so that
rem the build behaves the same from any shell. It finds the first full
rem distribution present and otherwise fails with a message naming the fix.
rem
rem To pin a specific interpreter, set OPENSSL_SRC_PERL_FULL to its perl.exe.
rem ─────────────────────────────────────────────────────────────────────────────
setlocal

if not "%OPENSSL_SRC_PERL_FULL%"=="" goto explicit

set "P=%USERPROFILE%\scoop\apps\perl\current\perl\bin\perl.exe"
if exist "%P%" goto run

set "P=%ProgramFiles%\Strawberry\perl\bin\perl.exe"
if exist "%P%" goto run

set "P=C:\Strawberry\perl\bin\perl.exe"
if exist "%P%" goto run

set "P=%ProgramFiles(x86)%\Strawberry\perl\bin\perl.exe"
if exist "%P%" goto run

set "P=%LOCALAPPDATA%\Programs\Strawberry\perl\bin\perl.exe"
if exist "%P%" goto run

1>&2 echo [agent2ssh] No full Perl found, and OpenSSL's Configure needs one.
1>&2 echo [agent2ssh] Git for Windows bundles a trimmed Perl that lacks
1>&2 echo [agent2ssh] Locale::Maketext::Simple, so it cannot be used here.
1>&2 echo [agent2ssh] Install one:  scoop install perl
1>&2 echo [agent2ssh] Or point OPENSSL_SRC_PERL_FULL at a perl.exe from
1>&2 echo [agent2ssh] Strawberry Perl and re-run the build.
exit /b 1

:explicit
set "P=%OPENSSL_SRC_PERL_FULL%"
if not exist "%P%" (
    1>&2 echo [agent2ssh] OPENSSL_SRC_PERL_FULL is set but not a file: %P%
    exit /b 1
)

:run
"%P%" %*
exit /b %ERRORLEVEL%
