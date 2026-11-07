using OpenCvSharp;
using Size = OpenCvSharp.Size;
using Point = OpenCvSharp.Point;

namespace OpenScan.Core;

public sealed record CorrectionResult(double Angle, double InkPercent, bool Blank, int Width, int Height);

/// All edits are derived from the original scan. No correction overwrites a source page.
public sealed class ImageProcessor
{
    public CorrectionResult Process(string source, string destination, ScanProfile profile, int extraRotation = 0)
    {
        profile.Validate();
        using (var header = System.Drawing.Image.FromFile(source))
        {
            if ((long)header.Width * header.Height > 60_000_000 || header.Width < 16 || header.Height < 16)
                throw new IOException("Image dimensions must be at least 16 × 16 and at most 60 million pixels.");
        }
        using var original = Cv2.ImRead(source, ImreadModes.Color);
        if (original.Empty()) throw new IOException("The scan could not be decoded as an image.");
        using var image = original.Clone();
        int rotate = (profile.Cleanup.RotateDegrees + extraRotation) % 360;
        double angle = profile.Cleanup.Deskew ? EstimateSkew(image) : 0;
        if (Math.Abs(angle) >= 0.15)
        {
            using var matrix = Cv2.GetRotationMatrix2D(new Point2f(image.Width / 2f, image.Height / 2f), angle, 1);
            // Expand the canvas so correcting a slanted page cannot cut its corners off.
            double radians = angle * Math.PI / 180;
            int w = (int)Math.Ceiling(image.Width * Math.Abs(Math.Cos(radians)) + image.Height * Math.Abs(Math.Sin(radians)));
            int h = (int)Math.Ceiling(image.Width * Math.Abs(Math.Sin(radians)) + image.Height * Math.Abs(Math.Cos(radians)));
            matrix.Set(0, 2, matrix.At<double>(0, 2) + (w - image.Width) / 2.0);
            matrix.Set(1, 2, matrix.At<double>(1, 2) + (h - image.Height) / 2.0);
            Cv2.WarpAffine(image, image, matrix, new Size(w, h), InterpolationFlags.Linear, BorderTypes.Constant, Scalar.White);
        }
        // Straighten the original text lines before a user rotates the page: rotating first
        // would turn horizontal text vertical and lose deskew on every 90-degree edit.
        if (rotate != 0) Cv2.Rotate(image, image, rotate switch { 90 => RotateFlags.Rotate90Clockwise, 180 => RotateFlags.Rotate180, _ => RotateFlags.Rotate90Counterclockwise });
        if (profile.Cleanup.RemoveBackground)
        {
            using var background = new Mat();
            Cv2.GaussianBlur(image, background, new Size(0, 0), Math.Max(9, Math.Min(image.Width, image.Height) / 60.0));
            Cv2.Divide(image, background, image, 255);
        }
        if (profile.Cleanup.Denoise) Cv2.MedianBlur(image, image, 3);
        if (profile.Cleanup.Sharpen)
        {
            using var blur = new Mat();
            Cv2.GaussianBlur(image, blur, new Size(0, 0), 1);
            Cv2.AddWeighted(image, 1.6, blur, -0.6, 0, image);
        }
        if (profile.Cleanup.Brightness != 0 || profile.Cleanup.Contrast != 0)
        {
            double alpha = Math.Pow(2, profile.Cleanup.Contrast / 50.0);
            image.ConvertTo(image, -1, alpha, profile.Cleanup.Brightness * 2.55 + 127.5 * (1 - alpha));
        }
        if (profile.Cleanup.AutoCrop) CropContent(image, profile.Dpi);
        if (profile.Color != ScanColor.Color) Cv2.CvtColor(image, image, ColorConversionCodes.BGR2GRAY);
        double ink = InkCoverage(image);
        if (profile.Color == ScanColor.BlackWhite)
            Cv2.AdaptiveThreshold(image, image, 255, AdaptiveThresholdTypes.GaussianC, ThresholdTypes.Binary, 41, 15);
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(destination))!);
        if (!Cv2.ImWrite(destination, image)) throw new IOException("Could not write the corrected page.");
        return new(angle, ink, ink < profile.Cleanup.BlankInkPercent, image.Width, image.Height);
    }

    public static double EstimateSkew(Mat input)
    {
        using var gray = Gray(input);
        using var small = new Mat();
        double scale = Math.Min(1, 1400.0 / Math.Max(gray.Width, gray.Height));
        Cv2.Resize(gray, small, new Size(), scale, scale, InterpolationFlags.Area);
        using var edges = new Mat();
        Cv2.Canny(small, edges, 60, 180);
        var lines = Cv2.HoughLinesP(edges, 1, Math.PI / 1800, 35, Math.Max(35, small.Width / 18.0), 15);
        var samples = lines.Select(line =>
        {
            double dx = line.P2.X - line.P1.X, dy = line.P2.Y - line.P1.Y;
            double angle = Math.Atan2(dy, dx) * 180 / Math.PI;
            if (angle > 90) angle -= 180;
            if (angle < -90) angle += 180;
            return (Angle: angle, Weight: Math.Sqrt(dx * dx + dy * dy));
        }).Where(x => Math.Abs(x.Angle) <= 12).OrderBy(x => x.Angle).ToList();
        if (samples.Count < 4) return 0; // Insufficient evidence: leave photographs and sparse pages alone.
        double half = samples.Sum(x => x.Weight) / 2, weight = 0;
        foreach (var sample in samples) { weight += sample.Weight; if (weight >= half) return sample.Angle; }
        return 0;
    }

    public static double InkCoverage(Mat input)
    {
        using var gray = Gray(input);
        int margin = Math.Max(1, Math.Min(gray.Width, gray.Height) / 100);
        using var center = new Mat(gray, new Rect(margin, margin, gray.Width - 2 * margin, gray.Height - 2 * margin));
        using var binary = new Mat();
        Cv2.Threshold(center, binary, 210, 255, ThresholdTypes.BinaryInv);
        return 100.0 * Cv2.CountNonZero(binary) / (center.Width * (double)center.Height);
    }

    private static Mat Gray(Mat input)
    {
        if (input.Channels() == 1) return input.Clone();
        var result = new Mat();
        Cv2.CvtColor(input, result, ColorConversionCodes.BGR2GRAY);
        return result;
    }

    private static void CropContent(Mat image, int dpi)
    {
        using var gray = Gray(image);
        using var mask = new Mat();
        Cv2.Threshold(gray, mask, 210, 255, ThresholdTypes.BinaryInv);
        Cv2.FindContours(mask, out Point[][] contours, out _, RetrievalModes.External, ContourApproximationModes.ApproxSimple);
        var regions = contours.Select(Cv2.BoundingRect).Where(r => r.Width * r.Height >= 12).ToList();
        if (regions.Count == 0) return;
        int margin = Math.Max(8, (int)Math.Round(dpi * 0.12));
        int left = Math.Max(0, regions.Min(r => r.Left) - margin), top = Math.Max(0, regions.Min(r => r.Top) - margin);
        int right = Math.Min(image.Width, regions.Max(r => r.Right) + margin), bottom = Math.Min(image.Height, regions.Max(r => r.Bottom) + margin);
        using var crop = new Mat(image, new Rect(left, top, right - left, bottom - top)).Clone();
        crop.CopyTo(image);
    }
}
