<?php

namespace App\Models;

use App\Contracts\Describable;

abstract class Model implements Describable
{
    /** @var array<string, mixed> */
    protected array $attributes = [];

    /**
     * @param array<string, mixed> $attributes
     */
    public static function create(array $attributes): static
    {
        $model = new static();
        $model->attributes = $attributes;
        return $model;
    }

    public function save(): bool
    {
        return $this->attributes !== [];
    }

    abstract public function describe(): string;
}
