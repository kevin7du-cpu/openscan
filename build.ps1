param(
    [string]$DotnetExecutable = 'dotnet',
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'dist\win-x64'),
    [switch]$DownloadOcr,
    [switch]$Test
)
$ErrorActionPreference = 'Stop'
$moduleRoot = $PSScriptRoot
if (!(Test-Path -LiteralPath (Join-Path $moduleRoot 'native\win-x64\OpenCvSharpExtern.dll'))) {
    throw 'Build the minimal image runtime first: .\native\build-native.ps1 (requires CMake and MinGW-w64). See README.md.'
}
$font = Join-Path $moduleRoot 'resources\fonts\Inter-Regular.ttf'
if (!(Test-Path -LiteralPath $font)) {
    New-Item -ItemType Directory -Path (Split-Path $font) -Force | Out-Null
    Invoke-WebRequest 'https://raw.githubusercontent.com/storytold/pdfcraft/5644d56c64af1529b0f9012874886fcad855c257/assets/fonts/Inter-Regular.ttf' -OutFile $font
}
if ((Get-FileHash -LiteralPath $font -Algorithm SHA256).Hash -ne '40D692FCE188E4471E2B3CBA937BE967878F631AD3EBBBDCD587687C7EBE0C82') {
    throw 'The OFL OCR font failed its SHA-256 check.'
}
$data = Join-Path $moduleRoot 'resources\tessdata\eng.traineddata'
if ($DownloadOcr -or !(Test-Path -LiteralPath $data)) {
    New-Item -ItemType Directory -Path (Split-Path $data) -Force | Out-Null
    Invoke-WebRequest 'https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/87416418657359cb625c412a48b6e1d6d41c29bd/eng.traineddata' -OutFile $data
}
if ((Get-FileHash -LiteralPath $data -Algorithm SHA256).Hash -ne '7D4322BD2A7749724879683FC3912CB542F19906C83BCC1A52132556427170B2') {
    throw 'The OCR language data failed its SHA-256 check.'
}
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$appProject = Join-Path $moduleRoot 'src\OpenScan.App\OpenScan.App.csproj'
$destination = [IO.Path]::GetFullPath($OutputDirectory)
& $DotnetExecutable publish $appProject -c Release -r win-x64 --self-contained true -o $destination --nologo
if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
foreach ($folder in @('tessdata', 'licenses')) {
    $source = if ($folder -eq 'tessdata') { Join-Path $moduleRoot 'resources\tessdata' } else { Join-Path $moduleRoot 'licenses' }
    $target = Join-Path $destination $folder
    New-Item -ItemType Directory -Path $target -Force | Out-Null
    Get-ChildItem -LiteralPath $source -File | Copy-Item -Destination $target -Force
}
Copy-Item -LiteralPath (Join-Path $moduleRoot 'LICENSE'),(Join-Path $moduleRoot 'THIRD-PARTY-NOTICES.md') -Destination $destination -Force
if ($Test) {
    $testDirectory = Join-Path $moduleRoot ('work\test-' + [guid]::NewGuid().ToString('N'))
    & $DotnetExecutable run --project (Join-Path $moduleRoot 'tests\OpenScan.Tests') -c Release -- $testDirectory
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed.' }
}
Write-Output "Built reusable Windows module: $destination"
