namespace OpenScan.Core;

public sealed record ScanResult(int Received, int Included, string? Warning);

public sealed class ScanService(IScannerBackend backend)
{
    public async Task<ScanResult> AcquireAsync(ScanSession session, ScanProfile profile,
        IProgress<ScanProgress>? progress = null, CancellationToken cancellation = default)
    {
        profile.Validate();
        if (session.Pages.Count + profile.MaxPages > 250)
            profile = profile with { MaxPages = 250 - session.Pages.Count };
        profile.Validate();
        var raw = new List<RawPage>();
        string incoming = Path.Combine(session.DirectoryPath, "incoming", Guid.NewGuid().ToString("N"));
        string? warning = null;
        // Acquisition callbacks only record filenames; processing never holds up the driver.
        try { await backend.ScanAsync(profile, incoming, raw.Add, progress, cancellation); }
        catch (OperationCanceledException) { warning = "Scanning cancelled. Pages already received were retained."; }
        catch (Exception e) { warning = e.Message; }
        await Task.Run(() =>
        {
            int i = 0;
            foreach (var page in raw)
            {
                progress?.Report(new("Correcting received pages", ++i, 100 * i / raw.Count));
                int rotation = profile.Source == ScanSource.Duplex && profile.FlipDuplexBacks && i % 2 == 0 ? 180 : 0;
                session.Add(page, profile, rotation);
            }
        });
        return new(raw.Count, session.Pages.Count(x => !x.Excluded), warning);
    }
}
