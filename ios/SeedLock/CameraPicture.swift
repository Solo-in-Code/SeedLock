import Foundation

/// The grey part of a camera picture (its Y plane: one byte per pixel) as SeedLock's QR reader takes it: `width`
/// bytes per row, row after row. In the camera's memory, each row may end with padding (`bytesPerRow` is then larger
/// than `width`); that is left out.
func packedLuma(_ base: UnsafeRawPointer, width: Int, height: Int, bytesPerRow: Int) -> Data {
    guard width > 0, height > 0, bytesPerRow >= width else {
        return Data()
    }
    if bytesPerRow == width {
        return Data(bytes: base, count: width * height)
    }
    var luma = Data(count: width * height)
    luma.withUnsafeMutableBytes { (out: UnsafeMutableRawBufferPointer) in
        guard let start = out.baseAddress else { return }
        for row in 0..<height {
            (start + row * width).copyMemory(from: base + row * bytesPerRow, byteCount: width)
        }
    }
    return luma
}
