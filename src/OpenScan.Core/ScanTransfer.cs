namespace OpenScan.Core;

/// Stream-to-file boundary shared by acquisition drivers. WIA can return a native IStream
/// positioned at EOF even when its managed wrapper reports Position=0.
public static class ScanTransfer
{
    public static void SavePage(Stream source, string destination)
    {
        if (source.CanSeek) source.Seek(0, SeekOrigin.Begin);
        using var output = new FileStream(destination, FileMode.CreateNew);
        byte[] buffer = new byte[81920];
        long total = 0;
        int read;
        while ((read = source.Read(buffer, 0, buffer.Length)) != 0)
        {
            total += read;
            if (total > 256L * 1024 * 1024) throw new IOException("The scan exceeds the 256 MB page limit. Lower the resolution.");
            output.Write(buffer, 0, read);
        }
        if (total == 0) throw new IOException("The scanner returned an empty image stream. Try its native interface.");
    }
}
