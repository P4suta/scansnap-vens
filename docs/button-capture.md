# Scan to a folder with the Scan button

Connect the ScanSnap iX1300 to the local network and pair it once with ScanSnap Home.
Open the scanner cover and load paper in the top chute.
Start the daemon with the scanner's address and an existing writable output folder.

```sh
mise x -- just vens discover
mise x -- cargo run --locked -p vens-daemon --bin vensd -- \
  --address 192.0.2.7 --consume "/path/to/consume"
```

Replace the example address with the address that discovery reports.
When the daemon logs `waiting for the Scan button`, press Scan once.
Each press saves the front and back of every sheet to one Portable Document Format (PDF) file, in feed order.
The daemon takes a reservation while it waits and shares it with eSCL clients.
The worker discards any press notifications that arrive during a scan.
Use the scanner from ScanSnap Home after you stop the daemon and the scanner releases its reservation.

The button notifications use unauthenticated User Datagram Protocol (UDP) messages.
Run the bridge on a trusted network segment and restrict access to its UDP trigger socket to the scanner.

Use the Paperless-ngx consumption directory as the output folder for automatic import.
[Paperless-ngx usage](https://docs.paperless-ngx.com/usage/#the-consumption-directory)

On Unix, each new PDF gives read access to the account that runs `vensd`.
Run `vensd` and the Paperless-ngx consumer with the same effective user ID.
Grant that user permission to create and remove files in the consumption directory.
The daemon writes a temporary file with a `.part` suffix, then publishes the complete file under a unique `.pdf` name in the same directory.
Each image keeps its original orientation.

Without `--consume`, the daemon sends pages to eSCL clients and reserves the scanner for requested jobs.
To advertise the bridge on Linux, use the service definition from `mise x -- just vensd avahi-service` with Avahi.
The eSCL client notes cover supported operations and hardware checks.
[Client behavior](escl-client-behavior.md)
