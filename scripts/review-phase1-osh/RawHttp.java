// Original diagnostic transport, Apache-2.0. No peer/server model imports.
import java.io.ByteArrayOutputStream;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.util.Base64;

/** Captures through EOF, including an illegal HEAD payload; never follows links. */
public final class RawHttp {
    public static void main(String[] args) throws Exception {
        if (args.length != 2) throw new IllegalArgumentException("port and request required");
        int port = Integer.parseInt(args[0]);
        if (port != 18080 && port != 18888) throw new IllegalArgumentException("unapproved port");
        byte[] request = Base64.getDecoder().decode(args[1]);
        if (request.length > 65536) throw new IllegalArgumentException("request cap");
        try (Socket socket = new Socket()) {
            socket.connect(new InetSocketAddress("127.0.0.1", port), 3000);
            socket.setSoTimeout(10000);
            socket.getOutputStream().write(request);
            socket.getOutputStream().flush();
            var bytes = new ByteArrayOutputStream();
            byte[] block = new byte[8192];
            int count;
            long deadline = System.nanoTime() + 15_000_000_000L;
            while ((count = socket.getInputStream().read(block)) != -1) {
                if (System.nanoTime() > deadline || bytes.size() + count > 1048576)
                    throw new IllegalStateException("incomplete capture: time/response cap");
                bytes.write(block, 0, count);
            }
            // A timeout/over-cap raises, never emits a truncated response as complete.
            System.out.print(Base64.getEncoder().encodeToString(bytes.toByteArray()));
        }
    }
}
