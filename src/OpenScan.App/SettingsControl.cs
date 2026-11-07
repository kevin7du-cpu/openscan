using OpenScan.Core;

namespace OpenScan.App;

internal sealed class SettingsControl : UserControl
{
    private readonly ComboBox devices = Combo();
    private readonly ComboBox source = Combo();
    private readonly ComboBox color = Combo();
    private readonly ComboBox paper = Combo();
    private readonly ComboBox dpi = Combo();
    private readonly NumericUpDown maxPages = Number(1, 250, 100);
    private readonly CheckBox native = Check("Use scanner's interface");
    private readonly CheckBox flipBacks = Check("Rotate duplex backs 180°");
    private readonly CheckBox deskew = Check("Straighten pages (deskew)", true);
    private readonly CheckBox crop = Check("Crop to content");
    private readonly CheckBox background = Check("Whiten uneven background");
    private readonly CheckBox denoise = Check("Remove dust / speckles");
    private readonly CheckBox sharpen = Check("Sharpen text");
    private readonly CheckBox blank = Check("Exclude blank pages (reversible)");
    private readonly CheckBox ocr = Check("Searchable PDF (English OCR)");
    private readonly NumericUpDown brightness = Number(-100, 100, 0);
    private readonly NumericUpDown contrast = Number(-100, 100, 0);
    private readonly NumericUpDown coverage = Number(0, 5, 0.08m, 2);
    private readonly NumericUpDown quality = Number(20, 100, 90);
    private readonly ComboBox rotate = Combo();
    private readonly Label note = new() { AutoSize = true, MaximumSize = new Size(330, 0), ForeColor = Color.DimGray, Padding = new Padding(0, 5, 0, 8) };
    private readonly IScannerBackend backend;
    private readonly FlowLayoutPanel fields = new() { Dock = DockStyle.Fill, FlowDirection = FlowDirection.TopDown, WrapContents = false, AutoScroll = true, Padding = new Padding(12) };
    private ScanProfile current = new();
    private bool applying;
    private int capabilitiesVersion;
    private ScannerCapabilities? capabilities;

    public SettingsControl(IScannerBackend backend)
    {
        this.backend = backend;
        Dock = DockStyle.Fill;
        Controls.Add(fields);
        Heading("Scanner");
        Add(devices);
        var refresh = Button("Refresh scanners", async () => await RefreshDevices());
        Add(refresh);
        Add(note);
        Add(native);
        Heading("Acquisition");
        Row("Source / sides", source);
        Row("Color", color);
        Row("Paper size", paper);
        Row("Resolution (DPI)", dpi);
        Row("Maximum pages", maxPages);
        Add(flipBacks);
        Heading("Image correction");
        foreach (var control in new[] { deskew, crop, background, denoise, sharpen, blank }) Add(control);
        Row("Blank ink %", coverage);
        Row("Brightness", brightness);
        Row("Contrast", contrast);
        Row("Rotate clockwise", rotate);
        Heading("Output");
        Add(ocr);
        Row("JPEG quality", quality);
        color.Items.AddRange(Enum.GetNames<ScanColor>());
        paper.Items.AddRange(Enum.GetNames<Paper>());
        source.Items.AddRange(Enum.GetNames<ScanSource>());
        dpi.Items.AddRange(new object[] { 75, 100, 150, 200, 300, 400, 600 });
        rotate.Items.AddRange(new object[] { 0, 90, 180, 270 });
        devices.DisplayMember = nameof(ScannerDevice.Name);
        devices.SelectedIndexChanged += async (_, _) => { if (!applying) await RefreshCapabilities(); };
        source.SelectedIndexChanged += (_, _) => { if (!applying) UpdateResolutions(); };
        native.CheckedChanged += (_, _) => AcquisitionEnabled();
        Apply(new ScanProfile());
    }

    public async Task RefreshDevices()
    {
        string? selected = (devices.SelectedItem as ScannerDevice)?.Id ?? current.DeviceId;
        note.Text = "Finding connected scanners…";
        try
        {
            var found = await backend.GetDevicesAsync();
            applying = true;
            devices.Items.Clear();
            foreach (var device in found) devices.Items.Add(device);
            devices.SelectedItem = found.FirstOrDefault(x => x.Id == selected);
            if (devices.SelectedIndex < 0 && selected == null && found.Count > 0) devices.SelectedIndex = 0;
            applying = false;
            if (found.Count == 0) note.Text = "No WIA scanner found. Power on the scanner and install its manufacturer's WIA driver, then refresh.";
            else if (devices.SelectedIndex < 0) note.Text = "The preset's scanner is disconnected. Select an available scanner.";
            else await RefreshCapabilities();
        }
        catch (Exception e) { applying = false; note.Text = e.Message; }
    }

    private async Task RefreshCapabilities()
    {
        int version = ++capabilitiesVersion;
        if (devices.SelectedItem is not ScannerDevice device) return;
        note.Text = "Reading scanner capabilities…";
        try
        {
            var caps = await backend.GetCapabilitiesAsync(device.Id);
            if (version != capabilitiesVersion || IsDisposed) return;
            capabilities = caps;
            string previous = source.SelectedItem?.ToString() ?? current.Source.ToString();
            applying = true;
            source.Items.Clear();
            foreach (var cap in caps.Sources) source.Items.Add(cap.Source.ToString());
            if (source.Items.Count == 0) { source.Items.Add(ScanSource.Flatbed.ToString()); native.Checked = true; }
            source.SelectedItem = previous;
            if (source.SelectedIndex < 0) source.SelectedIndex = 0;
            applying = false;
            UpdateResolutions();
            note.Text = caps.Note;
        }
        catch (Exception e) { applying = false; note.Text = e.Message + " You can try the scanner's interface."; }
    }

    private void UpdateResolutions()
    {
        var cap = capabilities?.Sources.FirstOrDefault(x => x.Source.ToString() == source.SelectedItem?.ToString());
        int previous = Convert.ToInt32(dpi.SelectedItem ?? current.Dpi);
        int[] choices = cap?.Dpi.Length > 0 ? cap.Dpi : new[] { 75, 100, 150, 200, 300, 400, 600 }
            .Where(x => cap == null || x >= cap.MinDpi && x <= cap.MaxDpi).ToArray();
        dpi.Items.Clear(); foreach (int value in choices) dpi.Items.Add(value);
        dpi.SelectedItem = previous;
        if (dpi.SelectedIndex < 0 && choices.Length > 0) dpi.SelectedItem = choices.OrderBy(x => Math.Abs(x - previous)).First();
    }

    public void Apply(ScanProfile profile)
    {
        profile.Validate();
        current = profile;
        applying = true;
        devices.SelectedItem = devices.Items.Cast<ScannerDevice>().FirstOrDefault(x => x.Id == profile.DeviceId);
        if (!source.Items.Contains(profile.Source.ToString())) source.Items.Add(profile.Source.ToString());
        source.SelectedItem = profile.Source.ToString();
        color.SelectedItem = profile.Color.ToString(); paper.SelectedItem = profile.Paper.ToString();
        if (!dpi.Items.Contains(profile.Dpi)) dpi.Items.Add(profile.Dpi);
        dpi.SelectedItem = profile.Dpi; maxPages.Value = profile.MaxPages;
        native.Checked = profile.NativeInterface; flipBacks.Checked = profile.FlipDuplexBacks;
        deskew.Checked = profile.Cleanup.Deskew; crop.Checked = profile.Cleanup.AutoCrop;
        background.Checked = profile.Cleanup.RemoveBackground; denoise.Checked = profile.Cleanup.Denoise;
        sharpen.Checked = profile.Cleanup.Sharpen; blank.Checked = profile.Cleanup.SkipBlankPages;
        coverage.Value = (decimal)profile.Cleanup.BlankInkPercent;
        brightness.Value = profile.Cleanup.Brightness; contrast.Value = profile.Cleanup.Contrast;
        rotate.SelectedItem = profile.Cleanup.RotateDegrees;
        ocr.Checked = profile.SearchablePdf; quality.Value = profile.JpegQuality;
        applying = false;
        AcquisitionEnabled();
    }

    public ScanProfile Read(string? name = null)
    {
        var profile = new ScanProfile
        {
            Name = name ?? current.Name, DeviceId = (devices.SelectedItem as ScannerDevice)?.Id,
            Source = Enum.Parse<ScanSource>(source.SelectedItem?.ToString() ?? "Flatbed"),
            Color = Enum.Parse<ScanColor>(color.SelectedItem?.ToString() ?? "Color"),
            Paper = Enum.Parse<Paper>(paper.SelectedItem?.ToString() ?? "Letter"),
            Dpi = Convert.ToInt32(dpi.SelectedItem ?? 300), MaxPages = (int)maxPages.Value,
            NativeInterface = native.Checked, FlipDuplexBacks = flipBacks.Checked,
            SearchablePdf = ocr.Checked, JpegQuality = (int)quality.Value,
            Cleanup = new CleanupOptions
            {
                Deskew = deskew.Checked, AutoCrop = crop.Checked, RemoveBackground = background.Checked,
                Denoise = denoise.Checked, Sharpen = sharpen.Checked, SkipBlankPages = blank.Checked,
                BlankInkPercent = (double)coverage.Value, Brightness = (int)brightness.Value,
                Contrast = (int)contrast.Value, RotateDegrees = Convert.ToInt32(rotate.SelectedItem ?? 0)
            }
        };
        profile.Validate();
        return profile;
    }

    private void AcquisitionEnabled()
    {
        foreach (Control control in new Control[] { source, color, paper, dpi, maxPages, flipBacks }) control.Enabled = !native.Checked;
    }
    private void Heading(string text) => Add(new Label { Text = text, AutoSize = true, Font = new Font(Font, FontStyle.Bold), Margin = new Padding(0, 16, 0, 7) });
    private void Add(Control control) { control.Margin = new Padding(0, 2, 0, 4); fields.Controls.Add(control); }
    private void Row(string text, Control control)
    {
        var row = new TableLayoutPanel { Width = 332, Height = 32, ColumnCount = 2, Margin = new Padding(0, 2, 0, 2) };
        row.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 53)); row.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 47));
        row.Controls.Add(new Label { Text = text, AutoSize = true, Anchor = AnchorStyles.Left }, 0, 0);
        control.Dock = DockStyle.Fill; row.Controls.Add(control, 1, 0); fields.Controls.Add(row);
    }
    private static ComboBox Combo() => new() { DropDownStyle = ComboBoxStyle.DropDownList, Width = 332, IntegralHeight = false, DropDownHeight = 250 };
    private static CheckBox Check(string text, bool value = false) => new() { Text = text, Checked = value, AutoSize = true, MaximumSize = new Size(330, 0) };
    private static NumericUpDown Number(decimal min, decimal max, decimal value, int decimals = 0) => new() { Minimum = min, Maximum = max, Value = value, DecimalPlaces = decimals, Increment = decimals > 0 ? 0.01m : 1 };
    internal static Button Button(string text, Action action)
    {
        var button = new Button { Text = text, AutoSize = true, MinimumSize = new Size(85, 32), FlatStyle = FlatStyle.System, Padding = new Padding(5, 0, 5, 0) };
        button.Click += (_, _) => action();
        return button;
    }
}
