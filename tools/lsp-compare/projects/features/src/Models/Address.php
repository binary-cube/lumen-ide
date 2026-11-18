<?php

namespace App\Models;

class Address
{
    public function __construct(
        public string $city = '',
        public string $street = '',
    ) {
    }

    public function format(): string
    {
        return $this->street . ', ' . $this->city;
    }
}
