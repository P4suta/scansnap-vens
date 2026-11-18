# Run the bridge

## Install the commands

Choose the archive for your platform when the [release](https://github.com/P4suta/scansnap-vens/releases) provides one.
The Linux archive targets x86-64 on Ubuntu 24.04, and the macOS archive targets Apple silicon.
Unpack the archive and place `vens` and `vensd` in a directory on your path.
For a source release, use the [source installation commands](../README.md#installation).

## Connect to the scanner

Connect the ScanSnap iX1300 to your local network and pair it once with ScanSnap Home.
Open the scanner cover and run `vens discover` to find its address.
In the README command, replace `192.0.2.7` with that address.
Replace `/path/to/consume` with an existing writable folder to save one PDF per scan.
The same daemon can serve eSCL clients while it waits for the physical button.
The bridge listens on port 8090.

The button guide explains how to use the Paperless-ngx consumption directory.
[Scan to a folder](button-capture.md)

On Linux, run `vensd avahi-service` to generate the service definition for Avahi.
Save the output as a `.service` file in `/etc/avahi/services` on the bridge host.
On macOS, advertise the bridge with Bonjour from the command below.

```sh
dns-sd -R "ScanSnap iX1300 (vensd)" _uscan._tcp local 8090 \
  txtvers=1 vers=2.0 rs=eSCL 'ty=ScanSnap iX1300' \
  uuid=9ff23966-94ee-48d8-9284-1e1e09ff7271 pdl=image/jpeg cs=color is=adf duplex=T
```

The client notes explain how Image Capture and sane-airscan use the bridge.
[eSCL client behavior](escl-client-behavior.md)

Run the workspace checks with `mise x -- just check`.
[Prepare and publish a release](releases.md)
