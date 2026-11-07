using System.Text.Json;
using OpenScan.Core;

namespace OpenScan.App;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            ApplicationConfiguration.Initialize();
            if (args.Length == 0 || args[0] is "dialog" or "presets-dialog")
            {
                if (args.FirstOrDefault() == "presets-dialog")
                {
                    Application.Run(new PresetWindow(new PresetStore(), new WiaScannerBackend()));
                    Print(new { ok = true, protocolVersion = 1 }); return 0;
                }
                int sessionFlag = Array.IndexOf(args, "--session");
                string? sessionPath = sessionFlag >= 0 ? args.ElementAtOrDefault(sessionFlag + 1) ?? throw new ArgumentException("--session requires a directory") : null;
                using var window = new ScanWindow(args.Contains("--return-json"), sessionPath);
                Application.Run(window);
                Print(new { ok = true, protocolVersion = 1, path = window.ResultPath, cancelled = window.ResultPath == null }); return 0;
            }
            return Command(args).GetAwaiter().GetResult();
        }
        catch (Exception e) { Print(new { ok = false, protocolVersion = 1, error = e.Message }); return 1; }
    }

    private static void Print(object result) => Console.WriteLine(JsonSerializer.Serialize(result, ScanJson.Options));
    private static T Input<T>() => JsonSerializer.Deserialize<T>(Console.In.ReadToEnd(), ScanJson.Options) ?? throw new ArgumentException("Expected a JSON object on standard input.");

    private static async Task<int> Command(string[] args)
    {
        var backend = new WiaScannerBackend();
        using var cancellation = new CancellationTokenSource();
        Console.CancelKeyPress += (_, e) => { e.Cancel = true; cancellation.Cancel(); };
        switch (args[0])
        {
            case "devices": Print(new { ok = true, protocolVersion = 1, devices = await backend.GetDevicesAsync(cancellation.Token) }); break;
            case "capabilities":
                var device = Input<DeviceRequest>();
                Print(new { ok = true, protocolVersion = 1, capabilities = await backend.GetCapabilitiesAsync(device.DeviceId, cancellation.Token) }); break;
            case "scan":
                var request = Input<ScanRequest>(); request.Profile.Validate();
                if (request.Profile.NativeInterface) throw new ArgumentException("Native scanner UI is only available through the Scan window.");
                if (File.Exists(request.Output)) throw new IOException("Output already exists. Choose a new filename.");
                if (request.SessionDirectory != null && Directory.Exists(request.SessionDirectory)) throw new IOException("Session directory already exists. Choose a new directory.");
                var session = new ScanSession(request.SessionDirectory);
                var result = await new ScanService(backend).AcquireAsync(session, request.Profile, cancellation: cancellation.Token);
                if (session.Pages.Any(p => !p.Excluded)) new PdfWriter().Export(session.Pages, request.Output, request.Profile, cancellation: cancellation.Token);
                Print(new { ok = result.Received > 0 && result.Warning == null, protocolVersion = 1, path = File.Exists(request.Output) ? Path.GetFullPath(request.Output) : null, session = session.DirectoryPath, result });
                return result.Warning == null && result.Received > 0 ? 0 : 2;
            case "process":
                var process = Input<ProcessRequest>(); process.Profile.Validate();
                if (process.Images.Length is < 1 or > 250) throw new ArgumentException("Provide 1–250 image paths.");
                if (File.Exists(process.Output)) throw new IOException("Output already exists. Choose a new filename.");
                var imported = new ScanSession();
                foreach (string path in process.Images) { cancellation.Token.ThrowIfCancellationRequested(); imported.Add(new(path, process.Profile.Dpi), process.Profile); }
                new PdfWriter().Export(imported.Pages, process.Output, process.Profile, cancellation: cancellation.Token);
                Print(new { ok = true, protocolVersion = 1, path = Path.GetFullPath(process.Output), session = imported.DirectoryPath, pages = imported.Pages }); break;
            case "presets": Print(new { ok = true, protocolVersion = 1, presets = new PresetStore(args.ElementAtOrDefault(1)).List() }); break;
            case "preset-list":
                var directory = Input<DirectoryRequest>();
                Print(new { ok = true, protocolVersion = 1, presets = new PresetStore(directory.Directory).List() }); break;
            case "preset-save":
                var save = Input<PresetRequest>(); new PresetStore(save.Directory).Save(save.Profile, save.Overwrite);
                Print(new { ok = true, protocolVersion = 1 }); break;
            case "preset-delete":
                var delete = Input<NameRequest>(); new PresetStore(delete.Directory).Delete(delete.Name);
                Print(new { ok = true, protocolVersion = 1 }); break;
            case "version": Print(new { ok = true, protocolVersion = 1, version = "0.1.0", drivers = new[] { "wia" }, ocr = new PdfWriter().OcrAvailable }); break;
            default:
                throw new ArgumentException("Commands: dialog [--return-json], presets-dialog, devices, capabilities, scan, process, presets, preset-save, preset-delete, version. Non-GUI requests use JSON on standard input.");
        }
        return 0;
    }
    private sealed record DeviceRequest(string DeviceId);
    private sealed record DirectoryRequest(string Directory);
    private sealed record ScanRequest(ScanProfile Profile, string Output, string? SessionDirectory = null);
    private sealed record ProcessRequest(string[] Images, ScanProfile Profile, string Output);
    private sealed record PresetRequest(ScanProfile Profile, bool Overwrite = false, string? Directory = null);
    private sealed record NameRequest(string Name, string? Directory = null);
}
