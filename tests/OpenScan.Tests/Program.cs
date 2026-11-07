using System.Security.Cryptography;
using OpenCvSharp;
using OpenScan.Core;
using PdfSharp.Pdf.IO;
using PdfSharp.Pdf;
using PdfSharp.Pdf.Advanced;
using Point = OpenCvSharp.Point;
using Size = OpenCvSharp.Size;

string root = args.FirstOrDefault() ?? Path.Combine(Path.GetTempPath(), "openscan-tests-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(root);
int passed = 0;
void Check(bool condition, string name) { if (!condition) throw new Exception("FAIL: " + name); Console.WriteLine("PASS: " + name); passed++; }
void Reject(Action action, string name) { try { action(); } catch (Exception) { Check(true, name); return; } Check(false, name); }
string nativeInfo = Cv2.GetBuildInformation();
File.WriteAllText(Path.Combine(root, "native-build-info.txt"), nativeInfo);
Check(nativeInfo.Contains("Non-free algorithms:         NO") && !nativeInfo.Contains("Intel IPP:") && !nativeInfo.Contains("FFMPEG:                  YES"), "minimal imaging runtime excludes nonfree, IPP and FFmpeg");

var processor = new ImageProcessor();
using (var transferred = new MemoryStream(new byte[] { 66, 77, 1, 2, 3 }))
{
    transferred.Position = transferred.Length;
    string transferPath = Path.Combine(root, "transfer.bin");
    ScanTransfer.SavePage(transferred, transferPath);
    Check(File.ReadAllBytes(transferPath).SequenceEqual(new byte[] { 66, 77, 1, 2, 3 }), "acquisition rewinds a stream positioned at EOF");
}
Reject(() => ScanTransfer.SavePage(new MemoryStream(), Path.Combine(root, "empty.bin")), "empty scanner image is rejected before processing");
string source = Path.Combine(root, "skewed.png");
using (var document = new Mat(1600, 1200, MatType.CV_8UC3, Scalar.White))
{
    Cv2.PutText(document, "OPEN SCAN TEST DOCUMENT", new Point(100, 130), HersheyFonts.HersheySimplex, 1.6, Scalar.Black, 3);
    for (int y = 240; y < 1400; y += 65)
        Cv2.PutText(document, "Reusable document scanning and correction 12345", new Point(100, y), HersheyFonts.HersheySimplex, 0.9, Scalar.Black, 2);
    using var matrix = Cv2.GetRotationMatrix2D(new Point2f(600, 800), -4.5, 1);
    using var tilted = new Mat();
    Cv2.WarpAffine(document, tilted, matrix, document.Size(), InterpolationFlags.Linear, BorderTypes.Constant, Scalar.White);
    Cv2.ImWrite(source, tilted);
}
byte[] before = SHA256.HashData(File.ReadAllBytes(source));
string corrected = Path.Combine(root, "corrected.png");
var clean = processor.Process(source, corrected, new ScanProfile());
Check(Math.Abs(clean.Angle - 4.5) < 1, "deskew detects a known 4.5-degree skew");
using (var actual = Cv2.ImRead(corrected)) Check(Math.Abs(ImageProcessor.EstimateSkew(actual)) < 0.6, "corrected page is horizontal");
Check(before.SequenceEqual(SHA256.HashData(File.ReadAllBytes(source))), "corrections preserve source bytes");
string blank = Path.Combine(root, "blank.png");
using (var white = new Mat(1000, 800, MatType.CV_8UC3, Scalar.White)) Cv2.ImWrite(blank, white);
var blankResult = processor.Process(blank, Path.Combine(root, "blank-result.png"), new ScanProfile());
Check(blankResult.Blank && blankResult.Angle == 0, "blank pages are detected without invented skew");
var crop = processor.Process(source, Path.Combine(root, "cropped.png"), new ScanProfile { Cleanup = new() { AutoCrop = true } });
Check(crop.Width < clean.Width && crop.Height < clean.Height, "content crop removes unused margins");
processor.Process(source, Path.Combine(root, "bw.png"), new ScanProfile { Color = ScanColor.BlackWhite, Cleanup = new() { RemoveBackground = true, Denoise = true, Sharpen = true } });
using (var bw = Cv2.ImRead(Path.Combine(root, "bw.png"), ImreadModes.Unchanged))
{
    Check(bw.Channels() == 1, "black and white output has one channel");
    using var notBlack = new Mat(); using var notWhite = new Mat(); using var middle = new Mat();
    Cv2.Compare(bw, 0, notBlack, CmpTypes.NE); Cv2.Compare(bw, 255, notWhite, CmpTypes.NE); Cv2.BitwiseAnd(notBlack, notWhite, middle);
    Check(Cv2.CountNonZero(middle) == 0, "black and white output is binary");
}
Reject(() => (new ScanProfile { Dpi = 12000 }).Validate(), "unreasonable resolution rejected");
Reject(() => (new ScanProfile { Cleanup = new() { BlankInkPercent = double.NaN } }).Validate(), "NaN threshold rejected");

var store = new PresetStore(Path.Combine(root, "presets"));
var profile = new ScanProfile { Name = "Receipts", Source = ScanSource.Duplex, Dpi = 200, Cleanup = new() { AutoCrop = true } };
store.Save(profile); Check(store.Get("receipts") == profile, "preset settings round-trip including cleanup");
Reject(() => store.Save(profile), "preset overwrite requires explicit permission");
store.Save(profile with { Dpi = 300 }, true); Check(store.Get("Receipts").Dpi == 300, "preset edit persists");
store.Save(profile with { Name = "../outside" }); Check(!File.Exists(Path.Combine(root, "outside.json")), "preset names cannot traverse directories");
store.Delete("Receipts"); Check(store.List().Count == 1, "preset delete removes only requested preset");

var session = new ScanSession(Path.Combine(root, "session"));
session.Add(new(source, 150), new ScanProfile());
session.Add(new(blank, 150), new ScanProfile { Cleanup = new() { SkipBlankPages = true } });
Check(session.Pages.Count == 2 && session.Pages[1].Excluded, "blank skipping keeps the recoverable original");
var loaded = ScanSession.Load(session.DirectoryPath); Check(loaded.Pages.SequenceEqual(session.Pages), "session recovery retains order and exclusion");
var originalPath = session.Pages[0].OriginalPath;
session.Reprocess(0, new ScanProfile(), 90);
Check(session.Pages[0].Rotation == 90 && File.Exists(originalPath), "page rotation is non-destructive");
using (var rotated = Cv2.ImRead(session.Pages[0].ImagePath))
{
    Cv2.Rotate(rotated, rotated, RotateFlags.Rotate90Counterclockwise);
    Check(Math.Abs(ImageProcessor.EstimateSkew(rotated)) < 0.6, "90-degree rotation retains deskew correction");
}
session.Reprocess(0, new ScanProfile(), 0);
string pdf = Path.Combine(root, "scan.pdf");
var writer = new PdfWriter { TessDataPath = Path.GetFullPath(Path.Combine(root, "..", "..", "resources", "tessdata")) };
writer.Export(session.Pages, pdf, new ScanProfile());
using (var doc = PdfReader.Open(pdf, PdfDocumentOpenMode.Import))
{
    Check(doc.PageCount == 1, "PDF omits excluded pages");
    using var pageImage = Cv2.ImRead(session.Pages[0].ImagePath);
    Check(Math.Abs(doc.Pages[0].Width.Point - pageImage.Width * 72.0 / 150) < 1, "PDF physical size respects scan DPI");
}
byte[] pdfBefore = File.ReadAllBytes(pdf);
Reject(() => writer.Export(session.Pages, pdf, new ScanProfile()), "existing PDF is not silently replaced");
Check(pdfBefore.SequenceEqual(File.ReadAllBytes(pdf)), "refused overwrite preserves existing PDF");
string appended = Path.Combine(root, "appended.pdf");
writer.Export(session.Pages, appended, new ScanProfile(), pdf);
using (var doc = PdfReader.Open(appended, PdfDocumentOpenMode.Import)) Check(doc.PageCount == 2, "append creates a two-page PDF copy");
Check(pdfBefore.SequenceEqual(File.ReadAllBytes(pdf)), "append preserves source PDF");
Reject(() => writer.Export(session.Pages, pdf, new ScanProfile(), pdf, true), "append cannot overwrite its own input");
string cancelled = Path.Combine(root, "cancelled.pdf");
Reject(() => writer.Export(session.Pages, cancelled, new ScanProfile(), cancellation: new CancellationToken(true)), "export honours cancellation");
Check(!File.Exists(cancelled), "cancelled export publishes no partial PDF");

var fakeSession = new ScanSession(Path.Combine(root, "partial"));
var result = await new ScanService(new JamAfterOne(source)).AcquireAsync(fakeSession, new ScanProfile());
Check(result.Received == 1 && result.Warning != null && fakeSession.Pages.Count == 1, "hardware failure preserves pages already received");
string ocrData = Path.Combine(AppContext.BaseDirectory, "tessdata");
writer = new PdfWriter { TessDataPath = ocrData };
string searchable = Path.Combine(root, "searchable.pdf");
writer.Export(session.Pages, searchable, new ScanProfile { SearchablePdf = true });
using (var doc = PdfReader.Open(searchable, PdfDocumentOpenMode.Import))
{
    var content = doc.Pages[0].Contents.CreateSingleContent().Stream.ToString();
    Check(content.Contains("BT") && content.Contains("Tj"), "OCR PDF contains a text layer");
    var fonts = doc.Pages[0].Elements.GetDictionary("/Resources").Elements.GetDictionary("/Font");
    var names = fonts.Elements.Values.OfType<PdfReference>().Select(r => ((PdfDictionary)r.Value).Elements.GetName("/BaseFont")).ToArray();
    Check(names.Length > 0 && names.All(name => name.Contains("Inter", StringComparison.OrdinalIgnoreCase)), "OCR embeds only the bundled OFL font");
}
Console.WriteLine($"{passed} checks passed. Test artifacts: {root}");

sealed class JamAfterOne(string path) : IScannerBackend
{
    public Task<IReadOnlyList<ScannerDevice>> GetDevicesAsync(CancellationToken cancellation = default) => Task.FromResult<IReadOnlyList<ScannerDevice>>([]);
    public Task<ScannerCapabilities> GetCapabilitiesAsync(string deviceId, CancellationToken cancellation = default) => throw new NotSupportedException();
    public Task ScanAsync(ScanProfile profile, string directory, Action<RawPage> onPage, IProgress<ScanProgress>? progress = null, CancellationToken cancellation = default)
    { onPage(new(path, 150)); throw new IOException("Simulated paper jam"); }
}
