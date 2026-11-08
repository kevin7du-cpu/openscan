param(
    [Parameter(Mandatory)][string]$PdfCraftCli,
    [Parameter(Mandatory)][string]$Scanner,
    [Parameter(Mandatory)][string]$Directory
)
$ErrorActionPreference='Stop'
foreach ($path in @($PdfCraftCli,$Scanner,$Directory)) {
    if (![IO.Path]::IsPathFullyQualified($path)) { throw 'Every test path must be absolute.' }
}
if (Test-Path -LiteralPath $Directory) { throw 'Use a fresh smoke-test directory.' }
New-Item -ItemType Directory -Path $Directory | Out-Null
$env:PDFCRAFT_SCANNER=$Scanner
Remove-Item Env:OPENSCAN_MODELS -ErrorAction SilentlyContinue
$results=@()

function Run-Process([string]$program,[string[]]$arguments,[string]$inputText='') {
    $start=[Diagnostics.ProcessStartInfo]::new($program)
    foreach($argument in $arguments) { $start.ArgumentList.Add($argument) }
    $start.UseShellExecute=$false
    $start.CreateNoWindow=$true
    $start.RedirectStandardInput=$true
    $start.RedirectStandardOutput=$true
    $start.RedirectStandardError=$true
    $process=[Diagnostics.Process]::Start($start)
    $process.StandardInput.Write($inputText)
    $process.StandardInput.Close()
    $stdout=$process.StandardOutput.ReadToEndAsync()
    $stderr=$process.StandardError.ReadToEndAsync()
    if (!$process.WaitForExit(180000)) { $process.Kill($true); throw 'Test child process timed out.' }
    [pscustomobject]@{code=$process.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result}
}
function Run-Steps([object[]]$steps,[string]$name,[bool]$success=$true) {
    $scriptPath=Join-Path $Directory ($name+'.json')
    ConvertTo-Json -InputObject $steps -Depth 12 | Set-Content -LiteralPath $scriptPath -Encoding utf8
    $process=Run-Process $PdfCraftCli @('run','--script',$scriptPath,'--root',$Directory,'--report')
    $report=$process.stdout | ConvertFrom-Json
    if (($process.code -eq 0) -ne $success -or [bool]$report.ok -ne $success) { throw ('Unexpected result: '+$name+' '+$process.stderr) }
    $process.stdout | Set-Content -LiteralPath (Join-Path $Directory ($name+'-result.json')) -Encoding utf8
    $report
}

$profile=@{name='Synthetic integration preset';dpi=150;cleanup=@{deskew=$true}}
$save=@{tool='scanner_preset_save';args=@{directory='presets';profile=$profile}}
$list=@{tool='scanner_presets';args=@{directory='presets'}}
$created=Run-Steps @($save,$list) 'create-list'
if ($created.steps[1].result[0].presets[0].dpi -ne 150) { throw 'Created preset did not round trip.' }
$results += 'Create and list preset'
$refused=Run-Steps @($save) 'refuse-overwrite' $false
$listed=Run-Steps @($list) 'after-refusal'
if ($listed.steps[0].result[0].presets[0].dpi -ne 150) { throw 'Refused overwrite changed the preset.' }
$results += 'Refuse implicit overwrite and preserve preset'
$updated=Run-Steps @(@{tool='scanner_preset_save';args=@{directory='presets';profile=@{name=$profile.name;dpi=300};overwrite=$true}},$list) 'update-list'
if ($updated.steps[1].result[0].presets[0].dpi -ne 300) { throw 'Explicit update did not round trip.' }
$results += 'Explicit update'
$deleted=Run-Steps @(@{tool='scanner_preset_delete';args=@{directory='presets';name=$profile.name}},$list) 'delete-list'
if (@($deleted.steps[1].result[0].presets).Count -ne 0) { throw 'Deleted preset remains listed.' }
$results += 'Delete and list empty'
$null=Run-Steps @(@{tool='scanner_preset_save';args=@{directory='../outside-test-root';profile=$profile}}) 'refuse-outside-root' $false
$results += 'Refuse a path outside the automation root'
$null=Run-Steps @(@{tool='scanner_preset_save';args=@{directory='presets';profile=@{name='Invalid DPI';dpi=0}}}) 'refuse-invalid-dpi' $false
$results += 'Structured error for invalid settings'

$models=Join-Path (Split-Path $Scanner) 'models'
New-Item -ItemType Directory -Force -Path $models | Out-Null
foreach ($model in @(
    @{name='text-detection.onnx';sha='a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe'},
    @{name='text-recognition.onnx';sha='86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5'}
)) {
    $modelPath=Join-Path $models $model.name
    if (!(Test-Path -LiteralPath $modelPath)) {
        Invoke-WebRequest -Uri ('https://ocrs-models.s3-accelerate.amazonaws.com/'+$model.name) -OutFile $modelPath
    }
    if ((Get-FileHash -LiteralPath $modelPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $model.sha) { throw 'OCR model hash differs.' }
}
[IO.File]::WriteAllText((Join-Path $Directory 'text.txt'),'OpenScan searchable document test. The quick brown fox jumps over the lazy dog. Invoice number 123456.',[Text.UTF8Encoding]::new($false))
$null=Run-Steps @(@{tool='doc_create_multiple';args=@{paths=@('text.txt');out='source.pdf';open=$true}}) 'create-ocr-source'
$sourcePdf=Join-Path $Directory 'source.pdf'
$sourcePng=Join-Path $Directory 'source.png'
$rendered=Run-Process $PdfCraftCli @('render',$sourcePdf,'--page','1','--dpi','150','--out',$sourcePng)
if ($rendered.code -ne 0) { throw 'Cannot render synthetic OCR input.' }
$outputPdf=Join-Path $Directory 'searchable.pdf'
$request=@{images=@($sourcePng);profile=@{name='Synthetic OCR test';dpi=150;searchablePdf=$true};output=$outputPdf} | ConvertTo-Json -Depth 6 -Compress
$processed=Run-Process $Scanner @('process') $request
$processReport=$processed.stdout | ConvertFrom-Json
if ($processed.code -ne 0 -or !$processReport.ok) { throw ('OCR failed: '+$processed.stdout+' '+$processed.stderr) }
$extracted=Run-Process $PdfCraftCli @('text',$outputPdf,'--page','1')
if ($extracted.code -ne 0 -or $extracted.stdout -notmatch 'quick brown fox' -or $extracted.stdout -notmatch '123456') { throw 'Searchable PDF failed independent extraction.' }
$rendered=Run-Process $PdfCraftCli @('render',$outputPdf,'--page','1','--dpi','96','--out',(Join-Path $Directory 'searchable.png'))
if ($rendered.code -ne 0) { throw 'Searchable PDF failed independent rendering.' }
$results += 'Packaged OCR models, PDF export, independent text extraction and rendering'
$summary=[ordered]@{ok=$true;checks=$results;passed=$results.Count;pdfcraftCliSha256=(Get-FileHash -LiteralPath $PdfCraftCli -Algorithm SHA256).Hash;scannerSha256=(Get-FileHash -LiteralPath $Scanner -Algorithm SHA256).Hash;physicalScanner=$false}
$summary | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $Directory 'summary.json') -Encoding utf8
$summary | ConvertTo-Json -Depth 5
