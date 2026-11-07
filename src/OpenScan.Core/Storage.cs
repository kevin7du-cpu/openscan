using System.Text.Json;

namespace OpenScan.Core;

public static class AtomicFile
{
    public static void Write(string path, Action<string> write, bool overwrite = false)
    {
        string full = Path.GetFullPath(path);
        string directory = Path.GetDirectoryName(full)!;
        Directory.CreateDirectory(directory);
        if (!overwrite && File.Exists(full)) throw new IOException("The destination exists. Choose another filename or explicitly allow replacement.");
        string temporary = Path.Combine(directory, $".{Path.GetFileName(full)}.{Guid.NewGuid():N}.tmp");
        try
        {
            using (new FileStream(temporary, FileMode.CreateNew)) { }
            write(temporary);
            File.Move(temporary, full, overwrite);
        }
        finally { if (File.Exists(temporary)) File.Delete(temporary); }
    }

    public static void Json<T>(string path, T value, bool overwrite = false) =>
        Write(path, temp => File.WriteAllText(temp, JsonSerializer.Serialize(value, ScanJson.Options)), overwrite);
}

public sealed class PresetStore
{
    public string DirectoryPath { get; }
    public PresetStore(string? directory = null)
    {
        DirectoryPath = directory ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "OpenScan", "Presets");
        Directory.CreateDirectory(DirectoryPath);
    }

    public IReadOnlyList<ScanProfile> List()
    {
        var list = new List<ScanProfile>();
        foreach (string path in Directory.GetFiles(DirectoryPath, "*.json"))
        {
            var profile = JsonSerializer.Deserialize<ScanProfile>(File.ReadAllText(path), ScanJson.Options)
                ?? throw new IOException($"Empty preset: {Path.GetFileName(path)}");
            profile.Validate();
            list.Add(profile);
        }
        return list.OrderBy(x => x.Name, StringComparer.OrdinalIgnoreCase).ToList();
    }

    private string PathFor(string name)
    {
        // Hash the logical name: user text can never escape the presets directory.
        string id = Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(name.Trim().ToUpperInvariant())));
        return Path.Combine(DirectoryPath, id + ".json");
    }

    public void Save(ScanProfile profile, bool overwrite = false)
    {
        profile = profile with { Name = profile.Name.Trim() };
        profile.Validate();
        AtomicFile.Json(PathFor(profile.Name), profile, overwrite);
    }

    public void Delete(string name) => File.Delete(PathFor(name));
    public ScanProfile Get(string name) => List().FirstOrDefault(x => x.Name.Equals(name, StringComparison.OrdinalIgnoreCase))
        ?? throw new ArgumentException($"Preset '{name}' was not found.");
}

public sealed class ScanSession
{
    public string DirectoryPath { get; }
    public List<PageRecord> Pages { get; } = [];
    public ScanSession(string? directory = null)
    {
        DirectoryPath = directory ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "OpenScan", "Sessions", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(DirectoryPath, "originals"));
        Directory.CreateDirectory(Path.Combine(DirectoryPath, "processed"));
    }

    public PageRecord Add(RawPage raw, ScanProfile profile, int rotation = 0)
    {
        string id = Guid.NewGuid().ToString("N");
        string original = Path.Combine(DirectoryPath, "originals", id + Path.GetExtension(raw.Path));
        File.Copy(raw.Path, original, false);
        string processed = Path.Combine(DirectoryPath, "processed", id + ".png");
        var result = new ImageProcessor().Process(original, processed, profile with { Dpi = raw.Dpi }, rotation);
        var page = new PageRecord(id, original, processed, raw.Dpi, result.Angle, result.InkPercent,
            profile.Cleanup.SkipBlankPages && result.Blank, rotation);
        Pages.Add(page);
        Save();
        return page;
    }

    public void Reprocess(int index, ScanProfile profile, int? rotation = null)
    {
        var page = Pages[index];
        int degrees = rotation ?? page.Rotation;
        // Unique output avoids replacing the currently previewed image and preserves the last good edit.
        string output = Path.Combine(DirectoryPath, "processed", Guid.NewGuid().ToString("N") + ".png");
        var result = new ImageProcessor().Process(page.OriginalPath, output, profile with { Dpi = page.Dpi }, degrees);
        Pages[index] = page with { ImagePath = output, DeskewAngle = result.Angle, InkPercent = result.InkPercent,
            Excluded = profile.Cleanup.SkipBlankPages && result.Blank, Rotation = degrees };
        Save();
    }

    public void Save() => AtomicFile.Json(Path.Combine(DirectoryPath, "session.json"), Pages, true);

    public static ScanSession Load(string directory)
    {
        string full = Path.GetFullPath(directory);
        var pages = JsonSerializer.Deserialize<List<PageRecord>>(File.ReadAllText(Path.Combine(full, "session.json")), ScanJson.Options)
            ?? throw new IOException("The session manifest is empty.");
        if (pages.Count > 250) throw new IOException("Session exceeds the 250-page limit.");
        var session = new ScanSession(full);
        foreach (var page in pages)
        {
            if (page.Dpi is < 75 or > 600 || page.Rotation is not (0 or 90 or 180 or 270)) throw new IOException("Invalid session page settings.");
            foreach (string path in new[] { page.OriginalPath, page.ImagePath })
                if (!Path.GetFullPath(path).StartsWith(full + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
                    throw new IOException("Session page paths must remain within the session directory.");
            session.Pages.Add(page);
        }
        return session;
    }
}
