from zeroconf import ServiceBrowser, ServiceListener, Zeroconf
from zeroconf._services.info import ServiceInfo


class MyListener(ServiceListener):

    def update_service(self, zc: Zeroconf, type_: str, name: str) -> None:
        print(f"Service {name} updated")

    def remove_service(self, zc: Zeroconf, type_: str, name: str) -> None:
        print(f"Service {name} removed")

    def add_service(self, zc: Zeroconf, type_: str, name: str) -> None:
        info = zc.get_service_info(type_, name)
        print(f"Service {name} added, service info: {info}")


zeroconf = Zeroconf()
listener = MyListener()
browser = ServiceBrowser(zeroconf, "_http._tcp.local.", listener)
zeroconf.register_service(ServiceInfo("_http._tcp.local.", "hrli_test._http._tcp.local.", 0))

try:
    input("Press enter to exit...\n\n")
finally:
    zeroconf.close()
