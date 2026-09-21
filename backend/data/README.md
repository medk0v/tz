# Visitor GeoIP data

`GeoLite2-City.mmdb` and `GeoLite2-Country.mmdb` were copied from the local
`exchange_platform` checkout on 2026-08-14. Tzomet uses them only for local,
coarse city/country enrichment of server-captured widget visitor IP addresses;
no external geolocation request is made.

Source product: MaxMind GeoLite2. Keep the two databases together when updating
them and follow the applicable MaxMind data-license and attribution terms.

SHA-256 checksums of the transferred files:

```text
9eff9b9aafe0122fbe04bf450b81913d8049632f0bd7833a3b9b3bc8794d654b  GeoLite2-City.mmdb
65a87a14454dbb2f2e87593b9b091575aeb0c2698c595289aba2f7456b64ad8a  GeoLite2-Country.mmdb
```
