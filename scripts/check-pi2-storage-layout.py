#!/usr/bin/env python3
"""Validate the kernel-storage layout against an existing image's MBR.

Read-only: this command never changes the image, card, or partition table.
"""
import argparse
import json
from pathlib import Path
import struct


def validate(layout, mbr, card_sectors):
    if layout['version'] != 1 or layout['sector_size'] != 512 or mbr[510:512] != b'\x55\xaa':
        raise ValueError('Unsupported layout or invalid MBR')
    partition = layout['kernel_partition']
    start, length = partition['start_sector'], partition['sector_count']
    if start < 1 or length <= 0 or start + length > card_sectors:
        raise ValueError('Kernel partition does not fit the card')
    entries = [mbr[446+i*16:462+i*16] for i in range(4)]
    preserved = {item['index']: item for item in layout['preserved_partitions']}
    for index, entry in enumerate(entries):
        existing_start, existing_length = struct.unpack_from('<II', entry, 8)
        if index in preserved:
            expected = preserved[index]
            if (entry[4], existing_start, existing_length) != (expected['type'], expected['start_sector'], expected['sector_count']):
                raise ValueError(f'Partition {index+1} differs from the preserved layout')
        elif index == partition['index']:
            if any(entry) and (entry[4], existing_start, existing_length) != (partition['type'], start, length):
                raise ValueError('Kernel partition entry is occupied by another layout')
            continue
        elif any(entry):
            raise ValueError(f'Partition {index+1} is occupied; refusing to assign it')
        if existing_length and start < existing_start + existing_length and existing_start < start + length:
            raise ValueError('Kernel area overlaps an existing partition')
    if partition['index'] in preserved or partition['index'] not in range(4):
        raise ValueError('Kernel partition requires a free MBR entry')
    ranges = [(0, partition['metadata_sectors'])]
    if not 0 <= partition['boot_record_offset'] < partition['metadata_sectors']:
        raise ValueError('Boot record lies outside metadata area')
    ranges += [(copy['offset_sectors'], copy['capacity_sectors']) for copy in partition['copies']]
    ranges += [(partition['spare_offset_sectors'], partition['spare_sectors'])]
    end = 0
    for offset, count in sorted(ranges):
        if offset < end or count <= 0 or offset + count > length:
            raise ValueError('Storage areas overlap or exceed the kernel partition')
        end = offset + count
    return start, start + length


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', type=Path, help='Existing recovery image; only its MBR is read')
    parser.add_argument('--layout', type=Path, default=Path(__file__).resolve().parents[1] / 'targets/armv7/boot/storage-layout.json')
    parser.add_argument('--card-sectors', type=int, required=True, help='Capacity reported by the physical card')
    args = parser.parse_args()
    with args.image.open('rb') as image:
        mbr = image.read(512)
    layout = json.loads(args.layout.read_text())
    start, end = validate(layout, mbr, args.card_sectors)
    print(f'PASS: sectors [{start}, {end}) fit without moving existing partitions; no writes performed')
    for copy in layout['kernel_partition']['copies']:
        print(f"Kernel {copy['name']}: start={start+copy['offset_sectors']}, capacity={copy['capacity_sectors']*512//(1024*1024)} MiB")
    print(f'Minimum image size: {end*512} bytes')


if __name__ == '__main__':
    main()
