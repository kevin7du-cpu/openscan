using OpenScan.Core;

namespace OpenScan.App;

internal sealed class ScanWindow : Form
{
    private readonly IScannerBackend backend = new WiaScannerBackend();
    private readonly PresetStore presets = new();
    private readonly ScanSession session;
    private readonly SettingsControl settings;
    private readonly ComboBox preset = new() { DropDownStyle = ComboBoxStyle.DropDownList, Width = 230, DisplayMember = nameof(ScanProfile.Name) };
    private readonly ListBox pages = new() { Dock = DockStyle.Left, Width = 170, IntegralHeight = false };
    private readonly PictureBox preview = new() { Dock = DockStyle.Fill, SizeMode = PictureBoxSizeMode.Zoom, BackColor = Color.FromArgb(224, 229, 235) };
    private readonly Label status = new() { Dock = DockStyle.Bottom, Height = 42, Padding = new Padding(12), Text = "Choose a scanner, or import an image to try correction tools." };
    private readonly Label pageDetails = new() { Dock = DockStyle.Bottom, Height = 44, Padding = new Padding(8) };
    private readonly FlowLayoutPanel actions = new() { Dock = DockStyle.Bottom, Height = 54, Padding = new Padding(8) };
    private readonly Button cancel;
    private readonly List<Control> lockedControls = [];
    private CancellationTokenSource? operation;
    private bool changed;
    private readonly bool returnOnExport;
    public string? ResultPath { get; private set; }

    public ScanWindow(bool returnOnExport = false, string? sessionDirectory = null)
    {
        this.returnOnExport = returnOnExport;
        session = sessionDirectory == null ? new ScanSession() : ScanSession.Load(sessionDirectory);
        Text = "Scan — OpenScan"; Size = new Size(1180, 840); MinimumSize = new Size(1000, 650);
        StartPosition = FormStartPosition.CenterScreen; Font = new Font("Segoe UI", 10);
        var menu = new MenuStrip();
        var fromScanner = new ToolStripMenuItem("From Scanner");
        fromScanner.DropDownItems.Add("Scan", null, async (_, _) => await Scan());
        fromScanner.DropDownItems.Add("Manage presets", null, (_, _) => ManagePresets());
        menu.Items.Add(fromScanner);
        var file = new ToolStripMenuItem("Session");
        file.DropDownItems.Add("Import images…", null, async (_, _) => await Import());
        file.DropDownItems.Add("Open saved session…", null, (_, _) => RestoreSession());
        file.DropDownItems.Add("Show session folder", null, (_, _) => System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(session.DirectoryPath) { UseShellExecute = true }));
        menu.Items.Add(file);
        var heading = new Label { Text = "Scan documents", Dock = DockStyle.Top, Height = 54, Padding = new Padding(12, 8, 0, 0), Font = new Font(Font.FontFamily, 20, FontStyle.Bold) };
        var presetBar = new FlowLayoutPanel { Dock = DockStyle.Top, Height = 50, Padding = new Padding(12, 6, 0, 0) };
        presetBar.Controls.Add(new Label { Text = "Preset", AutoSize = true, Padding = new Padding(0, 6, 5, 0) });
        presetBar.Controls.Add(preset);
        presetBar.Controls.Add(SettingsControl.Button("Save as preset", SavePreset));
        presetBar.Controls.Add(SettingsControl.Button("Manage presets", ManagePresets));
        settings = new SettingsControl(backend) { Dock = DockStyle.Left, Width = 374, BackColor = Color.FromArgb(248, 249, 251) };
        var workspace = new Panel { Dock = DockStyle.Fill, Padding = new Padding(8) };
        var tools = new FlowLayoutPanel { Dock = DockStyle.Top, Height = 48 };
        tools.Controls.Add(SettingsControl.Button("Rotate 90°", async () => await Rotate()));
        tools.Controls.Add(SettingsControl.Button("Move up", () => MovePage(-1)));
        tools.Controls.Add(SettingsControl.Button("Move down", () => MovePage(1)));
        tools.Controls.Add(SettingsControl.Button("Include / exclude", Toggle));
        tools.Controls.Add(SettingsControl.Button("Apply corrections", async () => await Reprocess()));
        workspace.Controls.Add(preview); workspace.Controls.Add(pages); workspace.Controls.Add(pageDetails); workspace.Controls.Add(tools);
        actions.Controls.Add(SettingsControl.Button("Scan / add pages", async () => await Scan()));
        actions.Controls.Add(SettingsControl.Button("Import images", async () => await Import()));
        actions.Controls.Add(SettingsControl.Button(returnOnExport ? "Save PDF and return" : "Save PDF", async () => await Export(false)));
        actions.Controls.Add(SettingsControl.Button("Append to PDF copy", async () => await Export(true)));
        actions.Controls.Add(SettingsControl.Button("Save images", async () => await ExportImages()));
        cancel = SettingsControl.Button("Cancel job", () => operation?.Cancel()); cancel.Enabled = false;
        actions.Controls.Add(cancel);
        Controls.Add(workspace); Controls.Add(settings); Controls.Add(presetBar); Controls.Add(heading); Controls.Add(actions); Controls.Add(status); Controls.Add(menu);
        MainMenuStrip = menu;
        lockedControls.AddRange([settings, presetBar, tools, pages, menu]);
        foreach (Control control in actions.Controls) if (control != cancel) lockedControls.Add(control);
        preset.SelectedIndexChanged += async (_, _) => { if (preset.SelectedItem is ScanProfile p) { settings.Apply(p); await settings.RefreshDevices(); } };
        pages.SelectedIndexChanged += (_, _) => ShowPreview();
        Shown += async (_, _) => { LoadPresets(); RefreshPages(); await settings.RefreshDevices(); };
        FormClosing += (_, e) =>
        {
            if (operation != null) { e.Cancel = true; operation.Cancel(); status.Text = "Cancelling… waiting for the scanner; received pages are retained."; return; }
            if (changed && MessageBox.Show(this, "Close without exporting the latest changes? Your pages are retained in the session folder and can be reopened.", "Close scan", MessageBoxButtons.YesNo) != DialogResult.Yes) e.Cancel = true;
        };
        FormClosed += (_, _) => preview.Image?.Dispose();
    }

    private void LoadPresets(string? selected = null)
    {
        try
        {
            preset.Items.Clear(); foreach (var p in presets.List()) preset.Items.Add(p);
            preset.SelectedItem = preset.Items.Cast<ScanProfile>().FirstOrDefault(p => p.Name == selected);
        }
        catch (Exception e) { ShowError(e); }
    }
    private void SavePreset()
    {
        string? name = NameDialog.Ask(this, "Save scan preset", "My scan preset"); if (name == null) return;
        try
        {
            bool exists = presets.List().Any(p => p.Name.Equals(name, StringComparison.OrdinalIgnoreCase));
            if (exists && MessageBox.Show(this, $"Replace preset '{name}'?", "Save preset", MessageBoxButtons.YesNo) != DialogResult.Yes) return;
            presets.Save(settings.Read(name), exists); LoadPresets(name);
        }
        catch (Exception e) { ShowError(e); }
    }
    private void ManagePresets() { using var form = new PresetWindow(presets, backend); form.ShowDialog(this); LoadPresets(); }

    private async Task Run(Func<CancellationToken, IProgress<ScanProgress>, Task> job)
    {
        if (operation != null) return;
        operation = new CancellationTokenSource();
        foreach (var control in lockedControls) control.Enabled = false;
        cancel.Enabled = true;
        var progress = new Progress<ScanProgress>(p => status.Text = $"{p.Stage} • page {p.Page} • {p.Percent}%");
        try { await job(operation.Token, progress); }
        catch (OperationCanceledException) { status.Text = "Cancelled. Received pages remain in this session."; }
        catch (Exception e) { ShowError(e); }
        finally
        {
            operation.Dispose(); operation = null;
            foreach (var control in lockedControls) control.Enabled = true;
            cancel.Enabled = false;
            RefreshPages();
        }
    }
    private async Task Scan()
    {
        ScanProfile profile;
        try { profile = settings.Read(); if (profile.DeviceId == null) throw new InvalidOperationException("Select a connected scanner first."); }
        catch (Exception e) { ShowError(e); return; }
        await Run(async (token, progress) =>
        {
            var result = await new ScanService(backend).AcquireAsync(session, profile, progress, token);
            changed |= result.Received > 0;
            status.Text = result.Warning ?? $"Received {result.Received} pages. {result.Included} included in the document. Scan again to add more pages.";
            if (result.Warning != null) MessageBox.Show(this, result.Warning, "Scanner status");
        });
    }
    private async Task Import()
    {
        using var dialog = new OpenFileDialog { Filter = "Images|*.png;*.jpg;*.jpeg;*.bmp;*.tif;*.tiff", Multiselect = true };
        if (dialog.ShowDialog(this) != DialogResult.OK) return;
        ScanProfile profile = settings.Read();
        await Run(async (token, progress) =>
        {
            await Task.Run(() =>
            {
                foreach (string path in dialog.FileNames)
                {
                    token.ThrowIfCancellationRequested();
                    if (session.Pages.Count >= 250) throw new IOException("The session has reached the 250-page limit.");
                    using var image = Image.FromFile(path);
                    var dimension = new System.Drawing.Imaging.FrameDimension(image.FrameDimensionsList[0]);
                    int frames = image.GetFrameCount(dimension);
                    for (int frame = 0; frame < frames; frame++)
                    {
                        token.ThrowIfCancellationRequested();
                        if (session.Pages.Count >= 250) throw new IOException("The session has reached the 250-page limit.");
                        image.SelectActiveFrame(dimension, frame);
                        int imageDpi = float.IsFinite(image.HorizontalResolution) && image.HorizontalResolution is >= 75 and <= 600 ? (int)Math.Round(image.HorizontalResolution) : profile.Dpi;
                        string incoming = Path.Combine(session.DirectoryPath, $"import-{Guid.NewGuid():N}.png");
                        image.Save(incoming, System.Drawing.Imaging.ImageFormat.Png);
                        session.Add(new(incoming, imageDpi), profile);
                        progress.Report(new("Importing and correcting", session.Pages.Count));
                    }
                }
            });
            changed = true; status.Text = "Images imported. Review pages, adjust settings, or export.";
        });
    }
    private async Task Reprocess()
    {
        ScanProfile profile = settings.Read();
        await Run(async (token, progress) =>
        {
            await Task.Run(() =>
            {
                for (int i = 0; i < session.Pages.Count; i++) { token.ThrowIfCancellationRequested(); session.Reprocess(i, profile); progress.Report(new("Applying corrections", i + 1)); }
            });
            changed = true; status.Text = "Corrections reapplied from the original scans.";
        });
    }
    private async Task Rotate()
    {
        int i = pages.SelectedIndex; if (i < 0) return;
        ScanProfile profile = settings.Read();
        await Run(async (_, _) => { await Task.Run(() => session.Reprocess(i, profile, (session.Pages[i].Rotation + 90) % 360)); changed = true; });
    }
    private void MovePage(int direction)
    {
        int i = pages.SelectedIndex, next = i + direction; if (i < 0 || next < 0 || next >= session.Pages.Count) return;
        (session.Pages[i], session.Pages[next]) = (session.Pages[next], session.Pages[i]); session.Save(); changed = true; RefreshPages(next);
    }
    private void Toggle()
    {
        int i = pages.SelectedIndex; if (i < 0) return;
        session.Pages[i] = session.Pages[i] with { Excluded = !session.Pages[i].Excluded }; session.Save(); changed = true; RefreshPages(i);
    }
    private void RefreshPages(int? selected = null)
    {
        int index = selected ?? pages.SelectedIndex;
        pages.Items.Clear();
        for (int i = 0; i < session.Pages.Count; i++) pages.Items.Add($"Page {i + 1}{(session.Pages[i].Excluded ? " — excluded" : "")}");
        if (pages.Items.Count > 0) pages.SelectedIndex = Math.Clamp(index < 0 ? pages.Items.Count - 1 : index, 0, pages.Items.Count - 1);
        else ShowPreview();
    }
    private void ShowPreview()
    {
        var previous = preview.Image; preview.Image = null; previous?.Dispose();
        int i = pages.SelectedIndex;
        if (i < 0) { pageDetails.Text = "Page preview — scans stay on this computer."; return; }
        var page = session.Pages[i];
        try { using var original = Image.FromFile(page.ImagePath); preview.Image = new Bitmap(original); }
        catch (Exception e) { status.Text = e.Message; }
        pageDetails.Text = $"Page {i + 1} of {session.Pages.Count} • {page.Dpi} DPI • deskew {page.DeskewAngle:F2}° • ink {page.InkPercent:F2}%{(page.Excluded ? " • EXCLUDED" : "")}";
    }
    private async Task Export(bool append)
    {
        if (!session.Pages.Any(p => !p.Excluded)) { status.Text = "Scan or import an included page first."; return; }
        string? source = null;
        if (append)
        {
            using var open = new OpenFileDialog { Filter = "PDF document|*.pdf", Title = "Choose the PDF to append to (a new copy will be saved)" };
            if (open.ShowDialog(this) != DialogResult.OK) return;
            source = open.FileName;
        }
        using var save = new SaveFileDialog { Filter = "PDF document|*.pdf", DefaultExt = "pdf", FileName = "Scanned document.pdf", OverwritePrompt = true };
        if (save.ShowDialog(this) != DialogResult.OK) return;
        ScanProfile profile = settings.Read();
        bool succeeded = false;
        await Run(async (token, progress) =>
        {
            await Task.Run(() => new PdfWriter().Export(session.Pages, save.FileName, profile, source, true, progress, token));
            ResultPath = save.FileName; changed = false; succeeded = true;
            status.Text = $"Saved {save.FileName}";
        });
        if (succeeded && returnOnExport) { DialogResult = DialogResult.OK; Close(); }
    }
    private async Task ExportImages()
    {
        if (!session.Pages.Any(p => !p.Excluded)) { status.Text = "Scan or import an included page first."; return; }
        using var folder = new FolderBrowserDialog { Description = "Choose the parent folder. A new scan-images folder will be created." };
        if (folder.ShowDialog(this) != DialogResult.OK) return;
        string path = Path.Combine(folder.SelectedPath, "scan-images-" + DateTime.Now.ToString("yyyyMMdd-HHmmss"));
        await Run(async (_, _) => { await Task.Run(() => PdfWriter.ExportImages(session.Pages, path, false)); status.Text = "Saved PNG pages to " + path; });
    }
    private void RestoreSession()
    {
        using var dialog = new OpenFileDialog { Filter = "Scan session|session.json", InitialDirectory = Path.GetDirectoryName(session.DirectoryPath) };
        if (dialog.ShowDialog(this) != DialogResult.OK) return;
        try { using var window = new ScanWindow(returnOnExport, Path.GetDirectoryName(dialog.FileName)); window.ShowDialog(this); if (window.ResultPath != null && returnOnExport) { ResultPath = window.ResultPath; changed = false; Close(); } }
        catch (Exception e) { ShowError(e); }
    }
    private void ShowError(Exception e) { status.Text = e.Message; MessageBox.Show(this, e.Message, "OpenScan", MessageBoxButtons.OK, MessageBoxIcon.Warning); }
}
