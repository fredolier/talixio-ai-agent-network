# Installe talixio-agent (Windows) : télécharge l'exécutable, vérifie son empreinte, le pose dans
# un dossier de l'utilisateur. Rien n'est lancé, rien ne demande de droits d'administrateur.
#   $env:TALIXIO_VERSION = "0.10.0"   une version précise (par défaut : la dernière)
#   $env:TALIXIO_INSTALL_DIR = "…"    où le poser (par défaut : %LOCALAPPDATA%\Talixio)
$ErrorActionPreference = "Stop"

& {
    $repo = "https://github.com/fredolier/talixio-ai-agent-network"
    if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") { throw "Processeur non pris en charge : $env:PROCESSOR_ARCHITECTURE." }
    $name = "talixio-agent-windows-x86_64.exe"
    $version = $env:TALIXIO_VERSION
    if (-not $version) { $base = "$repo/releases/latest/download" }
    elseif ($version -match '^[0-9.]+$') { $base = "$repo/releases/download/v$version" }
    else { throw "Version invalide : $version (attendu : 0.10.0)." }
    $dir = if ($env:TALIXIO_INSTALL_DIR) { $env:TALIXIO_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Talixio" }
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        Write-Host "Téléchargement de $name…"
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$name" -OutFile (Join-Path $tmp $name)
        Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile (Join-Path $tmp "SHA256SUMS")
        $line = Get-Content (Join-Path $tmp "SHA256SUMS") | Where-Object { ($_ -split '\s+')[1] -eq $name } | Select-Object -First 1
        $wanted = if ($line) { ($line -split '\s+')[0] } else { "" }
        $got = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $name)).Hash.ToLower()
        if (-not $wanted -or $wanted -ne $got) { throw "Empreinte inattendue pour $name : rien n'est installé." }
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $target = Join-Path $dir "talixio-agent.exe"
        Move-Item -Force (Join-Path $tmp $name) $target
        Write-Host "Installé : $target"
        Write-Host "Ensuite : écrivez talixio.toml (modèle : $base/talixio.example.toml), puis"
        Write-Host "  & `"$target`" root --config talixio.toml"
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}
