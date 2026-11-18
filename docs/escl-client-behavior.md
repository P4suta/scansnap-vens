# Client behavior that the eSCL bridge relies on

The bridge serves two clients: sane-airscan on Linux and Image Capture on macOS.
The Mopria specification of eSCL requires a license agreement, so the choices below rest on client source code, recorded client logs, and hardware runs.
The sane-airscan and AirSane code carries copyleft licenses, so it serves as evidence of behavior, and the bridge copies none of it.
Every sane-airscan link points at commit `689a6c6`.

## Documents

sane-airscan [matches elements by their prefixes](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-xml.c#L79-L94) instead of their namespaces.
The documents bind exactly the `pwg:` and `scan:` prefixes for that reason.
Each setting profile needs a color mode, a document format, and a resolution, or sane-airscan [rejects the whole capabilities document](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-escl.c#L400-L453).
sane-airscan [offers a gray mode of its own](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-devops.c#L76-L84) when a scanner offers `RGB24` alone.
It [fails on a one-component image](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-device.c#L1568-L1578) under `RGB24`.
In automatic color mode, the scanner returned three-component images for a color sheet, a blank sheet, and a black-text sheet on 2026-11-16.

## Job status

During a job, the status reports `Processing` and `ScannerAdfLoaded`.
sane-airscan [reads `ScannerAdfProcessing` as an empty feeder](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-escl.c#L1212-L1219) and gives up its retries.
After the last page, the status reports `Idle`, `ScannerAdfEmpty`, and `Completed` before the final 404.
sane-airscan [reads the status after that 404](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-escl.c#L1146-L1175) and treats `Processing` as a busy device.
An empty feeder in the status after a refused job request makes sane-airscan report that the feeder ran out of documents.
The bridge keeps that state for ten seconds, because sane-airscan checks the status before each job and wouldn't start while it lasts.

## Answers

sane-airscan [resolves the `Location` of a new job](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-escl.c#L985-L1002) relative to its base address.
A path in `Location` suffices, and AirSane's path served Image Capture in [AirSane issue 45](https://github.com/SimulPiscator/AirSane/issues/45).
sane-airscan [retries a job request after 503](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-escl.c#L1249-L1312) up to ten times, one second apart.
The bridge uses 503 for a busy scanner and 409 for a fault that a retry can't fix, as [go-mfp does](https://github.com/OpenPrinting/go-mfp/blob/b228e403d58ca8b70c0e279cb7d4ceeaff67be0c/proto/escl/abstractserver.go#L282-L298).
A job request that runs out of time before the worker can respond also gets 503.
On 2026-11-18 the bridge returned 503 while ScanSnap Home on Windows held the reservation.
After ScanSnap Home exited, the same bridge accepted another job and returned both sides of the sheet.
The log in AirSane issue 45 shows Image Capture waiting about five seconds for a held `NextDocument` answer.
The bridge holds that request until a page arrives, the job ends, the client disconnects, or the wait expires.
A matching `DELETE` ends the pending request with 404.
A newer `NextDocument` request ends the older one with 503 `Superseded`.
The bridge counts a page after the socket accepts the whole response and offers the same page again after a failed write.
The bridge keeps a drained job fault until the socket accepts the whole fault response.
The fault then stays visible for the feeder hint interval.

On 2026-11-18 Image Capture sent `Content-Type: text/xml` and saved both sides of an A4 sheet as images of 2592 by 3648 pixels.
The bridge accepts `text/xml` and `application/xml` for job requests, following the media types in [RFC 7303](https://www.rfc-editor.org/rfc/rfc7303#section-9).
With button capture enabled, another duplex scan returned both images and a final 404.
The worker then resumed waiting for the Scan button.

## Connections and advertisement

sane-airscan [sends `Connection: close`](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-http.c#L2253-L2270) with every request.
The bridge closes each connection after one answer, as [RFC 9112, section 9.3](https://www.rfc-editor.org/rfc/rfc9112#section-9.3) describes.
The bridge accepts numeric addresses, local hostnames, and `host.orb.internal` in the request authority.
Local hostnames include the `home.arpa` domain described in [RFC 8375](https://www.rfc-editor.org/rfc/rfc8375.html#section-4).
An acceptor delegates each connection to a bounded set of workers with separate request, response wait, and write deadlines.
sane-airscan [reads the `ty`, `uuid`, and `rs` keys](https://github.com/alexpevzner/sane-airscan/blob/689a6c660508fa94127318df8c6d6987e3d394cc/airscan-mdns.c#L586-L630) of the text record.
The record puts `txtvers` first, as [RFC 6763, section 6.7](https://www.rfc-editor.org/rfc/rfc6763#section-6.7) recommends.
It stays under the 400 bytes of section 6.2.
On 2026-11-18 a running Avahi daemon loaded the generated service file and resolved its `_uscan._tcp` record on port 8090.
The paper limits follow the U-turn limits in the [iX1300 specification](https://www.pfu.ricoh.com/global/scanners/scansnap/ix1300/).
The width limit is the 2592-pixel scan window at 300 dpi instead, because every image spans that window.
