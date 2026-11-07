using NAPS2.Wia;

namespace OpenScan.Core;

/// WIA COM objects are created, used and released on one dedicated STA thread per operation.
public sealed class WiaScannerBackend : IScannerBackend
{
    private const int ItemCategory = 4125;
    private const string FlatbedCategory = "fb607b1f-43f3-488b-855b-fb703ec342a6";
    private const string FeederCategory = "fe131934-f84c-42ad-8da4-6129cddd7288";
    private static readonly SemaphoreSlim Hardware = new(1, 1);

    private static Task<T> Sta<T>(Func<T> work, CancellationToken token)
    {
        var completion = new TaskCompletionSource<T>(TaskCreationOptions.RunContinuationsAsynchronously);
        var thread = new Thread(() =>
        {
            try { token.ThrowIfCancellationRequested(); completion.TrySetResult(work()); }
            catch (OperationCanceledException) { completion.TrySetCanceled(token.IsCancellationRequested ? token : new CancellationToken(true)); }
            catch (WiaException e) { completion.TrySetException(new IOException(Explain(e), e)); }
            catch (Exception e) { completion.TrySetException(e); }
        }) { IsBackground = true, Name = "OpenScan WIA" };
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        return completion.Task;
    }

    public Task<IReadOnlyList<ScannerDevice>> GetDevicesAsync(CancellationToken cancellation = default) => Sta<IReadOnlyList<ScannerDevice>>(() =>
    {
        using var manager = new WiaDeviceManager();
        var result = new List<ScannerDevice>();
        foreach (var info in manager.GetDeviceInfos())
        {
            using (info) result.Add(new ScannerDevice(info.Id(), info.Name()));
        }
        return result.OrderBy(x => x.Name).ToList();
    }, cancellation);

    private static bool IsSource(WiaItem item, ScanSource source)
    {
        var categoryProperty = item.Properties.GetOrNull(ItemCategory);
        // NAPS2.Wia 2.0.3 cannot read VT_CLSID (72). Most WIA2 drivers expose that type;
        // use their standard item name rather than throwing while enumerating capabilities.
        string category = categoryProperty?.Type == WiaPropertyType.BSTR ? categoryProperty.Value.ToString()?.Trim('{', '}') ?? "" : "";
        if (category.Equals(source == ScanSource.Flatbed ? FlatbedCategory : FeederCategory, StringComparison.OrdinalIgnoreCase)) return true;
        return item.Name().Contains(source == ScanSource.Flatbed ? "flatbed" : "feeder", StringComparison.OrdinalIgnoreCase);
    }

    public Task<ScannerCapabilities> GetCapabilitiesAsync(string deviceId, CancellationToken cancellation = default) => Sta(() =>
    {
        using var manager = new WiaDeviceManager();
        using var device = manager.FindDevice(deviceId);
        var items = device.GetSubItems();
        try
        {
            var sources = new List<SourceCapabilities>();
            foreach (var source in new[] { ScanSource.Flatbed, ScanSource.Feeder, ScanSource.Duplex })
            {
                var item = items.FirstOrDefault(x => IsSource(x, source));
                if (item == null) continue;
                int caps = Convert.ToInt32(device.Properties.GetOrNull(WiaPropertyId.DPS_DOCUMENT_HANDLING_CAPABILITIES)?.Value ?? 0);
                if (source == ScanSource.Duplex && (caps & WiaPropertyValue.DUPLEX) == 0) continue;
                var attr = item.Properties.GetOrNull(WiaPropertyId.IPS_XRES)?.Attributes;
                int[] resolutions = attr?.Values?.OfType<int>().Where(x => x is >= 75 and <= 600).ToArray() ?? [];
                sources.Add(new(source, resolutions, Math.Max(75, attr?.Min ?? 75), Math.Min(600, attr?.Max > 0 ? attr.Max : 600)));
            }
            return new ScannerCapabilities(deviceId, sources, sources.Count == 0
                ? "Driver did not report standard sources. Use the scanner's interface."
                : "Settings are checked again by the driver when scanning. Native interface settings override these controls.");
        }
        finally { foreach (var item in items) item.Dispose(); }
    }, cancellation);

    public async Task ScanAsync(ScanProfile profile, string rawDirectory, Action<RawPage> onPage,
        IProgress<ScanProgress>? progress = null, CancellationToken cancellation = default)
    {
        profile.Validate();
        if (string.IsNullOrWhiteSpace(profile.DeviceId)) throw new ArgumentException("Select a scanner first.");
        await Hardware.WaitAsync(cancellation);
        try
        {
            await Sta(() => Acquire(profile, rawDirectory, onPage, progress, cancellation), cancellation);
        }
        finally { Hardware.Release(); }
    }

    private static int Acquire(ScanProfile profile, string rawDirectory, Action<RawPage> onPage,
        IProgress<ScanProgress>? progress, CancellationToken cancellation)
    {
        Directory.CreateDirectory(rawDirectory);
        using var manager = new WiaDeviceManager();
        using var device = manager.FindDevice(profile.DeviceId!);
        if (profile.NativeInterface)
        {
            var files = manager.PromptForImage(device, tempFolder: rawDirectory) ?? [];
            foreach (string file in files.Take(profile.MaxPages))
            {
                cancellation.ThrowIfCancellationRequested();
                using var image = System.Drawing.Image.FromFile(file);
                int actualDpi = float.IsFinite(image.HorizontalResolution) && image.HorizontalResolution is >= 75 and <= 600
                    ? (int)Math.Round(image.HorizontalResolution) : profile.Dpi;
                onPage(new(file, actualDpi));
            }
            return files.Length;
        }
        var items = device.GetSubItems();
        try
        {
            var item = items.FirstOrDefault(x => IsSource(x, profile.Source))
                ?? throw new InvalidOperationException("The selected source is unavailable. Refresh the scanner or use its native interface.");
            int dpi = profile.Dpi;
            Required(item, WiaPropertyId.IPS_XRES, dpi);
            Required(item, WiaPropertyId.IPS_YRES, dpi);
            dpi = Convert.ToInt32(item.Properties[WiaPropertyId.IPS_XRES].Value);
            // Acquire RGB/gray; thresholding is software based, allowing non-destructive reprocessing.
            item.SetProperty(WiaPropertyId.IPA_DATATYPE, profile.Color == ScanColor.Color ? 3 : 2);
            item.SetProperty(WiaPropertyId.IPS_XPOS, 0);
            item.SetProperty(WiaPropertyId.IPS_YPOS, 0);
            var (width, height) = profile.PaperInches;
            Required(item, WiaPropertyId.IPS_XEXTENT, (int)Math.Round(width * dpi));
            Required(item, WiaPropertyId.IPS_YEXTENT, (int)Math.Round(height * dpi));
            if (profile.Source != ScanSource.Flatbed)
            {
                Required(item, WiaPropertyId.IPS_DOCUMENT_HANDLING_SELECT, profile.Source == ScanSource.Duplex ? WiaPropertyValue.DUPLEX : WiaPropertyValue.FRONT_ONLY);
                item.SetProperty(WiaPropertyId.IPS_PAGES, WiaPropertyValue.ALL_PAGES);
            }
            using var transfer = item.StartTransfer();
            int pages = 0;
            Exception? callbackError = null;
            transfer.Progress += (_, e) =>
            {
                if (cancellation.IsCancellationRequested) transfer.Cancel();
                progress?.Report(new("Scanning", pages + 1, e.Percent));
            };
            transfer.PageScanned += (_, e) =>
            {
                // Never let managed callback exceptions cross the native WIA callback boundary.
                using (e.Stream)
                {
                    try
                    {
                        cancellation.ThrowIfCancellationRequested();
                        string path = Path.Combine(rawDirectory, $"{Guid.NewGuid():N}.bmp");
                        ScanTransfer.SavePage(e.Stream, path);
                        pages++;
                        onPage(new(path, dpi));
                        if (pages >= profile.MaxPages) transfer.Cancel();
                    }
                    catch (Exception error) { callbackError = error; transfer.Cancel(); }
                }
            };
            transfer.Download();
            if (callbackError != null) throw callbackError;
            cancellation.ThrowIfCancellationRequested();
            if (pages == 0) throw new IOException("No pages were received. Load the scanner and try again.");
            return pages;
        }
        finally { foreach (var item in items) item.Dispose(); }
    }

    private static void Required(WiaItem item, int property, int value)
    {
        var prop = item.Properties.GetOrNull(property) ?? throw new IOException($"The driver does not expose property {property}. Use the scanner's interface.");
        prop.Value = value;
    }

    private static string Explain(WiaException e) => e.ErrorCode switch
    {
        0x80210002 => "Paper jam. Clear the scanner and retry; received pages are retained.",
        0x80210003 => "The feeder is empty. Load documents and try again.",
        0x80210005 => "The scanner is offline. Check its power and connection.",
        0x80210006 => "The scanner is busy in another application. Close that scan and retry.",
        0x80210016 => "The scanner cover is open. Close it and retry.",
        _ => $"Scanner error 0x{e.ErrorCode:X8}. Check the connection, paper and selected settings, or try the scanner's interface."
    };
}
