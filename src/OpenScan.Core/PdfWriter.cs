using System.Drawing.Imaging;
using PdfSharp.Drawing;
using PdfSharp.Pdf;
using PdfSharp.Pdf.IO;
using Tesseract;
using ImageFormat = System.Drawing.Imaging.ImageFormat;

namespace OpenScan.Core;

public sealed class PdfWriter
{
    public string TessDataPath { get; init; } = Path.Combine(AppContext.BaseDirectory, "tessdata");
    public bool OcrAvailable => File.Exists(Path.Combine(TessDataPath, "eng.traineddata"));

    public void Export(IReadOnlyList<PageRecord> pages, string destination, ScanProfile profile,
        string? appendFrom = null, bool overwrite = false, IProgress<ScanProgress>? progress = null, CancellationToken cancellation = default)
    {
        profile.Validate();
        var included = pages.Where(p => !p.Excluded).ToList();
        if (included.Count == 0) throw new InvalidOperationException("There are no included pages to export.");
        if (included.Count > 250) throw new InvalidOperationException("A document can contain up to 250 scanned pages.");
        if (appendFrom != null && Path.GetFullPath(appendFrom).Equals(Path.GetFullPath(destination), StringComparison.OrdinalIgnoreCase))
            throw new IOException("Save an appended PDF to a new filename to preserve the original document.");
        if (profile.SearchablePdf && !OcrAvailable) throw new IOException("English OCR data is missing. Run the build script with -DownloadOcr to install the free language data.");
        if (profile.SearchablePdf) OpenScanFonts.Register();
        using var ocr = profile.SearchablePdf ? new TesseractEngine(TessDataPath, "eng", EngineMode.Default) : null;
        AtomicFile.Write(destination, temporary =>
        {
            using var document = new PdfDocument();
            document.Info.Creator = "OpenScan";
            document.Info.Title = Path.GetFileNameWithoutExtension(destination);
            if (appendFrom != null)
            {
                using var existing = PdfReader.Open(appendFrom, PdfDocumentOpenMode.Import);
                foreach (var existingPage in existing.Pages) { cancellation.ThrowIfCancellationRequested(); document.AddPage(existingPage); }
            }
            int count = 0;
            foreach (var record in included)
            {
                cancellation.ThrowIfCancellationRequested();
                progress?.Report(new(profile.SearchablePdf ? "Recognizing text and exporting" : "Exporting PDF", ++count, 100 * count / included.Count));
                using var bitmap = new Bitmap(record.ImagePath);
                var page = document.AddPage();
                double unit = 72.0 / record.Dpi;
                page.Width = XUnit.FromPoint(bitmap.Width * unit);
                page.Height = XUnit.FromPoint(bitmap.Height * unit);
                using var graphics = XGraphics.FromPdfPage(page);
                if (ocr != null) AddText(graphics, ocr, record.ImagePath, unit, cancellation);
                // The opaque page image covers the text layer; text remains selectable/searchable.
                using var encoded = new MemoryStream();
                if (profile.Color == ScanColor.BlackWhite) bitmap.Save(encoded, ImageFormat.Png);
                else
                {
                    var codec = ImageCodecInfo.GetImageEncoders().First(c => c.FormatID == ImageFormat.Jpeg.Guid);
                    using var parameters = new EncoderParameters(1);
                    parameters.Param[0] = new EncoderParameter(System.Drawing.Imaging.Encoder.Quality, (long)profile.JpegQuality);
                    bitmap.Save(encoded, codec, parameters);
                }
                encoded.Position = 0;
                using var image = XImage.FromStream(encoded);
                graphics.DrawImage(image, 0, 0, page.Width.Point, page.Height.Point);
            }
            cancellation.ThrowIfCancellationRequested();
            document.Save(temporary);
        }, overwrite);
    }

    private static void AddText(XGraphics graphics, TesseractEngine engine, string path, double unit, CancellationToken cancellation)
    {
        using var pix = Pix.LoadFromFile(path);
        using var result = engine.Process(pix);
        using var iterator = result.GetIterator();
        iterator.Begin();
        do
        {
            cancellation.ThrowIfCancellationRequested();
            string? word = iterator.GetText(PageIteratorLevel.Word)?.Trim();
            if (string.IsNullOrEmpty(word) || !iterator.TryGetBoundingBox(PageIteratorLevel.Word, out var box) || box.Height < 1) continue;
            var font = new XFont(OpenScanFonts.Family, Math.Max(2, box.Height * unit), XFontStyleEx.Regular);
            double measured = graphics.MeasureString(word, font).Width;
            if (measured <= 0) continue;
            var state = graphics.Save();
            graphics.TranslateTransform(box.X1 * unit, box.Y1 * unit);
            graphics.ScaleTransform(box.Width * unit / measured, 1);
            graphics.DrawString(word, font, XBrushes.Black, new XPoint(0, 0), XStringFormats.TopLeft);
            graphics.Restore(state);
        } while (iterator.Next(PageIteratorLevel.Word));
    }

    public static void ExportImages(IReadOnlyList<PageRecord> pages, string directory, bool jpeg, int quality = 90)
    {
        if (Directory.Exists(directory)) throw new IOException("Choose a new folder for image export; existing folders are preserved.");
        Directory.CreateDirectory(directory);
        int index = 0;
        foreach (var page in pages.Where(x => !x.Excluded))
        {
            using var bitmap = new Bitmap(page.ImagePath);
            bitmap.SetResolution(page.Dpi, page.Dpi);
            string path = Path.Combine(directory, $"page-{++index:D4}.{(jpeg ? "jpg" : "png")}");
            if (!jpeg) bitmap.Save(path, ImageFormat.Png);
            else
            {
                using var parameters = new EncoderParameters(1);
                parameters.Param[0] = new EncoderParameter(System.Drawing.Imaging.Encoder.Quality, (long)quality);
                bitmap.Save(path, ImageCodecInfo.GetImageEncoders().First(c => c.FormatID == ImageFormat.Jpeg.Guid), parameters);
            }
        }
    }
}
