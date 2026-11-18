# Run the bridge

Connect the ScanSnap iX1300 to your local network and pair it once with ScanSnap Home.
Open the scanner cover and run `vens discover` to find its address.
In the README command, replace `192.0.2.7` with that address.
The bridge listens on port 8090.

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
