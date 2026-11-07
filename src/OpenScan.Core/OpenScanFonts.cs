using PdfSharp.Fonts;

namespace OpenScan.Core;

/// <summary>OFL-licensed font for the searchable PDF layer, independent of installed fonts.</summary>
public sealed class OpenScanFonts : IFontResolver
{
    public const string Family = "OpenScan OCR Inter";
    private const string Face = "openscan-inter-regular";
    private static readonly object Gate = new();
    private readonly IFontResolver? next;

    public OpenScanFonts(IFontResolver? next = null) => this.next = next;

    /// <summary>Call at host startup, before any other PDFsharp font is created.</summary>
    public static void Register()
    {
        lock (Gate)
        {
            if (GlobalFontSettings.FontResolver is OpenScanFonts) return;
            try { GlobalFontSettings.FontResolver = new OpenScanFonts(GlobalFontSettings.FontResolver); }
            catch (InvalidOperationException ex)
            {
                throw new InvalidOperationException("Register OpenScanFonts at application startup before using PDFsharp fonts, or use the OpenScan process interface.", ex);
            }
        }
    }

    public FontResolverInfo? ResolveTypeface(string familyName, bool bold, bool italic) =>
        familyName.Equals(Family, StringComparison.OrdinalIgnoreCase)
            ? new FontResolverInfo(Face)
            : next?.ResolveTypeface(familyName, bold, italic) ?? PlatformFontResolver.ResolveTypeface(familyName, bold, italic);

    public byte[]? GetFont(string faceName)
    {
        if (faceName != Face) return next?.GetFont(faceName);
        using var stream = typeof(OpenScanFonts).Assembly.GetManifestResourceStream("OpenScan.Inter-Regular.ttf")
            ?? throw new IOException("The bundled OFL OCR font is missing. Rebuild OpenScan with build.ps1.");
        using var bytes = new MemoryStream();
        stream.CopyTo(bytes);
        return bytes.ToArray();
    }
}
