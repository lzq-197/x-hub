/**
 * Session ids with an in-flight `kb_ask`. Module-scoped so it survives
 * KnowledgeView remount when the user switches sidebar views mid-generation.
 */
export const kbAskInFlight = new Set<number>()
