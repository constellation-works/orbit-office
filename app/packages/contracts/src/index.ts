/**
 * Types shared by the server and the web UI. Types only: the web UI
 * imports this package with `import type`, so the browser never has to
 * resolve it (ARCHITECTURE.md, "Web app"). Office tool input and output
 * types join here, generated from the plugin's JSON Schemas, as tools are
 * built.
 */
export type {
  AgentCliStatus,
  ApiCall,
  ConnectionInput,
  ConnectionSaved,
  ConnectionWrite,
  FeatureName,
  Health,
  EndpointKind,
  PrivateRouteBlock,
  PrivateRouteCheck,
  PrivateRouteCheckLine,
  PrivateRouteSaved,
  PrivateRouteStatus,
  PrivateRouteWrite,
  RouteCapability,
  ScreenId,
  SettingsStatus,
  SettingsConnection,
  SettingsConnections,
  SettingsApiCalls,
  Status,
  Today,
  TodayItem,
  TodayNeed,
  TodayResult,
  TodoItem,
  TodoList,
} from "./api.js";
