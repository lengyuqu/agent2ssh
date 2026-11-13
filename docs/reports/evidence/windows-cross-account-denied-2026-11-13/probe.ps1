param([string]$OutFile = "C:\Users\Administrator\AppData\Local\Temp\denied-probe-result.txt")
$src = @'
using System;
using System.Runtime.InteropServices;
public class P {
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll")]
  public static extern bool CloseHandle(IntPtr h);
}
'@
Add-Type -TypeDefinition $src
$lines = @()
$lines += "whoami: $(whoami)"
$lines += "is-elevated: $([Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))"
foreach ($name in @('winlogon','services','lsass')) {
  $proc = Get-Process -Name $name -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $proc) { $lines += "$name`: not found"; continue }
  $err = 0
  $h = [IntPtr]::Zero
  try {
    $h = [P]::OpenProcess(0x1000, $false, [uint32]$proc.Id)
    if ($h -eq [IntPtr]::Zero) {
      $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
      $lines += ("{0} pid={1} OpenProcess(QUERY_LIMITED_INFORMATION) DENIED err={2}" -f $name, $proc.Id, $err)
    } else {
      $lines += ("{0} pid={1} OpenProcess(QUERY_LIMITED_INFORMATION) OK" -f $name, $proc.Id)
      [void][P]::CloseHandle($h)
    }
  } catch {
    $lines += ("{0} pid={1} exception {2}" -f $name, $proc.Id, $_.Exception.Message)
  }
}
$lines | Set-Content -Path $OutFile -Encoding UTF8
