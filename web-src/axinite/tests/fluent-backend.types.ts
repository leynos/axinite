/** @file Type contract for the installed Fluent backend's module surfaces. */
import type { BackendModule } from "i18next";
import FluentBackend from "i18next-fluent-backend";

const backend: BackendModule = new FluentBackend();
const instanceType: "backend" = backend.type;
const constructorType: "backend" = FluentBackend.type;
void instanceType;
void constructorType;
