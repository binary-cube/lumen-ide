<?php

namespace App\Concerns;

trait HasTimestamps
{
    private ?\DateTimeImmutable $touchedAt = null;

    public function touch(): void
    {
        $this->touchedAt = new \DateTimeImmutable();
    }

    public function touchedAt(): ?\DateTimeImmutable
    {
        return $this->touchedAt;
    }
}
