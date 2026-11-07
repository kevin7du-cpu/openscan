param(
    [string]$CMake = 'cmake',
    [string]$ToolchainBin,
    [string]$WorkDirectory = (Join-Path $PSScriptRoot '..\work\native'),
    [int]$Jobs = 6
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ($ToolchainBin) { $env:PATH = $ToolchainBin + ';' + $env:PATH }
$work = [IO.Path]::GetFullPath($WorkDirectory)
New-Item -ItemType Directory -Path $work -Force | Out-Null
$sharpCommit = 'b161e7e012f5101f6d5dc68a835c59db6cc88b18'
foreach ($item in @(
    @{ Name='opencv'; Url='https://codeload.github.com/opencv/opencv/zip/refs/tags/4.13.0'; Hash='431CB489D275DA26526581C6735D7D7C0B00F190FE940B3217D386B77AEF52C7'; Folder='opencv-4.13.0' },
    @{ Name='opencvsharp'; Url="https://codeload.github.com/shimat/opencvsharp/zip/$sharpCommit"; Hash='3827D4B2EC9FDE0D3DF9120B388AFAEFEC17C7A03C1509B0EF6570D17C3FA4D4'; Folder="opencvsharp-$sharpCommit" }
)) {
    $zip = Join-Path $work ($item.Name + '.zip')
    if (!(Test-Path -LiteralPath $zip)) { Invoke-WebRequest $item.Url -OutFile $zip }
    if ((Get-FileHash -LiteralPath $zip).Hash -ne $item.Hash) { throw "Source checksum mismatch: $($item.Name)" }
    if (!(Test-Path -LiteralPath (Join-Path $work $item.Folder))) { Expand-Archive -LiteralPath $zip -DestinationPath $work }
}
$opencv = Join-Path $work 'opencv-4.13.0'
$build = Join-Path $work 'opencv-build'
$install = Join-Path $work 'opencv-install'
& $CMake -S $opencv -B $build -G 'MinGW Makefiles' -DCMAKE_BUILD_TYPE=Release "-DCMAKE_INSTALL_PREFIX=$install" `
    '-DBUILD_LIST=core,imgproc,imgcodecs' -DBUILD_SHARED_LIBS=OFF -DBUILD_TESTS=OFF -DBUILD_PERF_TESTS=OFF `
    -DBUILD_EXAMPLES=OFF -DBUILD_opencv_apps=OFF -DBUILD_JAVA=OFF -DBUILD_opencv_python2=OFF -DBUILD_opencv_python3=OFF `
    -DWITH_IPP=OFF -DWITH_ITT=OFF -DWITH_OPENCL=OFF -DWITH_FFMPEG=OFF -DWITH_MSMF=OFF -DWITH_DSHOW=OFF -DWITH_ADE=OFF -DBUILD_opencv_gapi=OFF `
    -DWITH_TIFF=OFF -DWITH_WEBP=OFF -DWITH_OPENJPEG=OFF -DWITH_JASPER=OFF -DWITH_OPENEXR=OFF -DWITH_AVIF=OFF `
    -DWITH_GTK=OFF -DWITH_QT=OFF -DWITH_PROTOBUF=OFF -DBUILD_PROTOBUF=OFF -DWITH_LAPACK=OFF -DWITH_EIGEN=OFF `
    -DOPENCV_ENABLE_NONFREE=OFF -DWITH_JPEG=ON -DBUILD_JPEG=ON -DBUILD_ZLIB=ON -DBUILD_PNG=ON `
    -DCPU_DISPATCH= -DCPU_BASELINE=SSE2
if ($LASTEXITCODE -ne 0) { throw 'OpenCV configure failed.' }
& $CMake --build $build --parallel $Jobs
if ($LASTEXITCODE -ne 0) { throw 'OpenCV build failed.' }
& $CMake --install $build
if ($LASTEXITCODE -ne 0) { throw 'OpenCV install failed.' }
$externBuild = Join-Path $work 'extern-build'
& $CMake -S (Join-Path $work "opencvsharp-$sharpCommit\src") -B $externBuild -G 'MinGW Makefiles' `
    -DCMAKE_BUILD_TYPE=Release "-DOpenCV_DIR=$install" -DNO_INSTALL_TO_TEST=ON `
    -DNO_CONTRIB=ON -DNO_STITCHING=ON -DNO_CALIB3D=ON -DNO_VIDEO=ON -DNO_FEATURES2D=ON -DNO_FLANN=ON `
    -DNO_DNN=ON -DNO_ML=ON -DNO_OBJDETECT=ON -DNO_PHOTO=ON -DNO_BARCODE=ON -DNO_HIGHGUI=ON -DNO_VIDEOIO=ON `
    '-DCMAKE_SHARED_LINKER_FLAGS=-static-libgcc -static-libstdc++' -DCMAKE_DISABLE_FIND_PACKAGE_Iconv=ON
if ($LASTEXITCODE -ne 0) { throw 'OpenCvSharp configure failed.' }
& $CMake --build $externBuild --parallel $Jobs
if ($LASTEXITCODE -ne 0) { throw 'OpenCvSharp build failed.' }
$destination = Join-Path $PSScriptRoot 'win-x64'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
$dll = Get-ChildItem -LiteralPath $externBuild -Recurse -File -Filter '*OpenCvSharpExtern.dll' | Select-Object -First 1
if (!$dll) { throw 'Native image runtime was not produced.' }
Copy-Item -LiteralPath $dll.FullName -Destination (Join-Path $destination 'OpenCvSharpExtern.dll') -Force
$runtimeBin = if ($ToolchainBin) { $ToolchainBin } else { Split-Path (Get-Command g++ -ErrorAction Stop).Source }
$pthread = Join-Path $runtimeBin 'libwinpthread-1.dll'
if (!(Test-Path -LiteralPath $pthread)) { throw 'The MinGW-w64 winpthreads runtime is missing from the compiler bin directory.' }
Copy-Item -LiteralPath $pthread -Destination $destination -Force
Write-Output "Built minimal image runtime: $destination"
