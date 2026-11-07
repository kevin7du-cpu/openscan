using System.Text.Json;
using System.Text.Json.Serialization;

namespace OpenScan.Core;

public enum ScanSource { Flatbed, Feeder, Duplex }
public enum ScanColor { Color, Grayscale, BlackWhite }
public enum Paper { Letter, A4, Legal, A5 }

public sealed record CleanupOptions
{
    public bool Deskew { get; init; } = true;
    public bool AutoCrop { get; init; }
    public bool RemoveBackground { get; init; }
    public bool Denoise { get; init; }
    public bool Sharpen { get; init; }
    public bool SkipBlankPages { get; init; }
    public double BlankInkPercent { get; init; } = 0.08;
    public int Brightness { get; init; }
    public int Contrast { get; init; }
    public int RotateDegrees { get; init; }
}

public sealed record ScanProfile
{
    public int SchemaVersion { get; init; } = 1;
    public string Name { get; init; } = "Document";
    public string? DeviceId { get; init; }
    public ScanSource Source { get; init; }
    public ScanColor Color { get; init; } = ScanColor.Color;
    public Paper Paper { get; init; } = Paper.Letter;
    public int Dpi { get; init; } = 300;
    public int MaxPages { get; init; } = 100;
    public bool NativeInterface { get; init; }
    public bool FlipDuplexBacks { get; init; }
    public bool SearchablePdf { get; init; }
    public int JpegQuality { get; init; } = 90;
    public CleanupOptions Cleanup { get; init; } = new();

    public void Validate()
    {
        if (SchemaVersion != 1) throw new ArgumentException("Unsupported profile version; expected 1.");
        if (string.IsNullOrWhiteSpace(Name) || Name.Length > 80) throw new ArgumentException("Preset name must contain 1–80 characters.");
        if (!Enum.IsDefined(Source) || !Enum.IsDefined(Color) || !Enum.IsDefined(Paper)) throw new ArgumentException("Invalid source, color or paper size.");
        if (Dpi is < 75 or > 600) throw new ArgumentException("Choose a resolution between 75 and 600 DPI.");
        if (MaxPages is < 1 or > 250) throw new ArgumentException("A batch can contain 1–250 pages.");
        if (JpegQuality is < 20 or > 100) throw new ArgumentException("JPEG quality must be 20–100.");
        if (Cleanup is null) throw new ArgumentException("Cleanup settings are required.");
        if (Cleanup.Brightness is < -100 or > 100 || Cleanup.Contrast is < -100 or > 100) throw new ArgumentException("Brightness and contrast must be -100–100.");
        if (Cleanup.RotateDegrees is not (0 or 90 or 180 or 270)) throw new ArgumentException("Rotation must be 0, 90, 180 or 270 degrees.");
        if (!double.IsFinite(Cleanup.BlankInkPercent) || Cleanup.BlankInkPercent is < 0 or > 5) throw new ArgumentException("Blank coverage must be 0–5 percent.");
    }

    public (double Width, double Height) PaperInches => Paper switch
    {
        Paper.A4 => (210 / 25.4, 297 / 25.4), Paper.A5 => (148 / 25.4, 210 / 25.4),
        Paper.Legal => (8.5, 14), _ => (8.5, 11)
    };
}

public sealed record ScannerDevice(string Id, string Name, string Driver = "WIA");
public sealed record SourceCapabilities(ScanSource Source, int[] Dpi, int MinDpi, int MaxDpi);
public sealed record ScannerCapabilities(string DeviceId, IReadOnlyList<SourceCapabilities> Sources, string Note);
public sealed record RawPage(string Path, int Dpi);
public sealed record ScanProgress(string Stage, int Page, int Percent = 0);
public sealed record PageRecord(string Id, string OriginalPath, string ImagePath, int Dpi, double DeskewAngle, double InkPercent, bool Excluded, int Rotation = 0)
{
    public override string ToString() => $"{(Excluded ? "[Excluded] " : "")}{Id[..8]}  •  {Dpi} DPI";
}

/// Driver plug-in boundary. Implement this interface to add TWAIN, eSCL or another OS.
public interface IScannerBackend
{
    Task<IReadOnlyList<ScannerDevice>> GetDevicesAsync(CancellationToken cancellation = default);
    Task<ScannerCapabilities> GetCapabilitiesAsync(string deviceId, CancellationToken cancellation = default);
    Task ScanAsync(ScanProfile profile, string rawDirectory, Action<RawPage> onPage,
        IProgress<ScanProgress>? progress = null, CancellationToken cancellation = default);
}

public static class ScanJson
{
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase, PropertyNameCaseInsensitive = true,
        WriteIndented = true, Converters = { new JsonStringEnumConverter(JsonNamingPolicy.CamelCase) },
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
    };
}
