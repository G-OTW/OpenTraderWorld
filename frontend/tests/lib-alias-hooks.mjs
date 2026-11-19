const LIB = new URL('../src/lib/', import.meta.url);

export async function resolve(specifier, context, next) {
  if (specifier.startsWith('$lib/')) return next(new URL(specifier.slice(5), LIB).href, context);
  return next(specifier, context);
}
