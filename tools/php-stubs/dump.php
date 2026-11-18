<?php
// Lumen's PHP stub generator: lists what the running PHP provides (functions, classes,
// interfaces, traits, enums, their methods and constants), by reflection, in Lumen's own
// line format. Nothing is copied from third-party stub files.
//
// Runs on PHP 5.6 and newer (so the same script can be run on every version and the
// results compared to know when each name appeared): no scalar types, no `??`.
//
// Output (UTF-8, tab-separated, one record per line):
//   v <version>
//   f <name> <params> <return> <extension>
//   c <kind> <name> <parent> <interfaces, comma-separated> <extension>
//   m <class> <name> <modifiers> <params> <return>
//   k <name> <extension>            a global constant
//   cc <class> <name>               a class constant
//   p <class> <name> <modifiers> <type>   a property the class declares
//   df <name>                       a deprecated function
//   dm <class> <name>               a deprecated method
// <extension> is the lowercase name of the extension providing it (`core`, `redis`…).

function lumen_type($type) {
    if ($type === null) {
        return '';
    }
    if (method_exists($type, 'getName') && !method_exists($type, 'getTypes')) {
        $name = $type->getName();
        return ($type->allowsNull() && $name !== 'mixed' && $name !== 'null' ? '?' : '') . $name;
    }
    return (string) $type;
}

function lumen_default(ReflectionParameter $p) {
    if (!$p->isDefaultValueAvailable()) {
        return $p->isOptional() ? ' = …' : '';
    }
    if (method_exists($p, 'isDefaultValueConstant') && $p->isDefaultValueConstant()) {
        return ' = ' . $p->getDefaultValueConstantName();
    }
    // Some defaults can't be evaluated outside their extension's context.
    try {
        $value = $p->getDefaultValue();
    } catch (Exception $e) {
        return ' = …';
    } catch (Throwable $e) {
        return ' = …';
    }
    if (is_array($value)) {
        return ' = []';
    }
    return ' = ' . str_replace(array("\t", "\n"), array('\t', '\n'), var_export($value, true));
}

function lumen_params($f) {
    $out = array();
    foreach ($f->getParameters() as $p) {
        $type = method_exists($p, 'getType') ? lumen_type($p->getType()) : '';
        $s = ($type !== '' ? $type . ' ' : '')
            . ($p->isPassedByReference() ? '&' : '')
            . ($p->isVariadic() ? '...' : '')
            . '$' . $p->getName();
        if (!$p->isVariadic()) {
            $s .= lumen_default($p);
        }
        $out[] = $s;
    }
    return implode(', ', $out);
}

function lumen_return($f) {
    if (method_exists($f, 'hasReturnType') && $f->hasReturnType()) {
        return lumen_type($f->getReturnType());
    }
    if (method_exists($f, 'hasTentativeReturnType') && $f->hasTentativeReturnType()) {
        return lumen_type($f->getTentativeReturnType());
    }
    return '';
}

function lumen_extension($reflection) {
    $name = $reflection->getExtensionName();
    return $name ? strtolower($name) : 'core';
}

echo "v\t" . PHP_MAJOR_VERSION . '.' . PHP_MINOR_VERSION . "\n";

$functions = get_defined_functions();
$names = $functions['internal'];
sort($names);
foreach ($names as $name) {
    $f = new ReflectionFunction($name);
    echo "f\t" . $f->getName() . "\t" . lumen_params($f) . "\t" . lumen_return($f)
        . "\t" . lumen_extension($f) . "\n";
    if ($f->isDeprecated()) {
        echo "df\t" . $f->getName() . "\n";
    }
}

$types = array_merge(get_declared_classes(), get_declared_interfaces(), get_declared_traits());
sort($types);
foreach ($types as $type) {
    $c = new ReflectionClass($type);
    if (!$c->isInternal()) {
        continue;
    }
    if (method_exists($c, 'isEnum') && $c->isEnum()) {
        $kind = 'enum';
    } elseif ($c->isInterface()) {
        $kind = 'interface';
    } elseif ($c->isTrait()) {
        $kind = 'trait';
    } else {
        $kind = 'class';
    }
    $parent = $c->getParentClass();
    echo "c\t$kind\t" . $c->getName() . "\t" . ($parent ? $parent->getName() : '')
        . "\t" . implode(',', $c->getInterfaceNames()) . "\t" . lumen_extension($c) . "\n";
    foreach ($c->getMethods() as $m) {
        if ($m->getDeclaringClass()->getName() !== $c->getName()) {
            continue;
        }
        $mods = implode(' ', Reflection::getModifierNames($m->getModifiers()));
        echo "m\t" . $c->getName() . "\t" . $m->getName() . "\t$mods\t" . lumen_params($m)
            . "\t" . lumen_return($m) . "\n";
        if ($m->isDeprecated()) {
            echo "dm\t" . $c->getName() . "\t" . $m->getName() . "\n";
        }
    }
    foreach ($c->getProperties() as $p) {
        if ($p->getDeclaringClass()->getName() !== $c->getName()) {
            continue;
        }
        $mods = implode(' ', Reflection::getModifierNames($p->getModifiers()));
        $type = method_exists($p, 'getType') ? lumen_type($p->getType()) : '';
        echo "p\t" . $c->getName() . "\t" . $p->getName() . "\t$mods\t$type\n";
    }
    foreach (array_keys($c->getConstants()) as $constant) {
        echo "cc\t" . $c->getName() . "\t$constant\n";
    }
}

$constants = get_defined_constants(true);
unset($constants['user']);
$all = array();
foreach ($constants as $extension => $group) {
    foreach (array_keys($group) as $name) {
        $all[$name] = strtolower($extension);
    }
}
ksort($all);
foreach ($all as $name => $extension) {
    echo "k\t$name\t$extension\n";
}
