<?php

namespace App\Enums;

enum Status: string
{
    case Active = 'active';
    case Banned = 'banned';

    public function label(): string
    {
        return ucfirst($this->value);
    }
}
