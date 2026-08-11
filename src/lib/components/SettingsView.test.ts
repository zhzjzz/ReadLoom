import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { defaultAppSettings } from '../stores/appSettings';
import SettingsView from './SettingsView.svelte';

describe('SettingsView', () => {
  it('presents the complete settings hierarchy requested by the reader', () => {
    render(SettingsView, {
      settings: defaultAppSettings,
      theme: 'system',
      headingPattern: '^第.+章',
    });

    const navigation = screen.getByRole('complementary', { name: '设置分类' });
    for (const group of ['外观', '阅读', '操作', '书籍', '数据', '高级']) {
      expect(within(navigation).getByRole('heading', { name: group })).toBeTruthy();
    }
    for (const item of ['主题', '字体', '页面布局', '阅读排版', '快捷键', '章节识别', '备份', '文件关联', '缓存', '硬件加速', 'DPI']) {
      expect(within(navigation).getByRole('button', { name: item })).toBeTruthy();
    }
    for (const removed of ['阅读区域', '翻页', '滚动', '阅读进度', '鼠标', '鼠标侧键', '书库目录', '文件扫描', 'TXT 编码', '文本净化', '阅读记录', '同步']) {
      expect(within(navigation).queryByRole('button', { name: removed })).toBeNull();
    }
  });

  it('changes library columns, background, and tray behavior from page layout', async () => {
    const onSettingsChange = vi.fn();
    const onChooseBackground = vi.fn();
    const onClearBackground = vi.fn();
    render(SettingsView, {
      settings: defaultAppSettings,
      theme: 'system',
      headingPattern: '^第.+章',
      backgroundKey: 'a'.repeat(64),
      backgroundUrl: `http://readloom-background.localhost/${'a'.repeat(64)}`,
      onSettingsChange,
      onChooseBackground,
      onClearBackground,
    });

    await fireEvent.click(screen.getByRole('button', { name: '页面布局' }));
    await fireEvent.click(screen.getByRole('radio', { name: '5 本' }));
    await fireEvent.click(screen.getByRole('button', { name: '选择图片' }));
    await fireEvent.click(screen.getByRole('button', { name: '清除背景' }));
    await fireEvent.click(screen.getByRole('radio', { name: '最小化到托盘' }));
    await fireEvent.click(screen.getByRole('checkbox'));

    expect(onSettingsChange).toHaveBeenCalledWith({ libraryColumns: 5 });
    expect(onSettingsChange).toHaveBeenCalledWith({ closeAction: 'tray' });
    expect(onSettingsChange).toHaveBeenCalledWith({ minimizeToTray: true });
    expect(onChooseBackground).toHaveBeenCalledOnce();
    expect(onClearBackground).toHaveBeenCalledOnce();
  });

  it('reuses the same background controls in Theme and Page Layout', async () => {
    const onSettingsChange = vi.fn();
    const onChooseBackground = vi.fn();
    render(SettingsView, {
      settings: defaultAppSettings,
      theme: 'system',
      headingPattern: '^第.+章',
      onSettingsChange,
      onChooseBackground,
    });

    await fireEvent.click(screen.getByRole('button', { name: '主题' }));
    await fireEvent.click(screen.getByRole('button', { name: '选择图片' }));
    const intensity = screen.getByLabelText('背景显示强度');
    expect(intensity.getAttribute('min')).toBe('0');
    expect(intensity.getAttribute('max')).toBe('100');
    await fireEvent.input(intensity, { target: { value: '35' } });

    expect(onChooseBackground).toHaveBeenCalledOnce();
    expect(onSettingsChange).toHaveBeenCalledWith({ backgroundOpacity: 0.35 });
  });

  it('keeps font choices while removing the free-font explanation heading', async () => {
    render(SettingsView, {
      settings: defaultAppSettings,
      theme: 'system',
      headingPattern: '^第.+章',
    });

    await fireEvent.click(screen.getByRole('button', { name: '字体' }));

    expect(screen.queryByText('常用免费阅读字体')).toBeNull();
    expect(screen.getByRole('button', { name: /系统默认/ })).toBeTruthy();
    expect(screen.getByRole('button', { name: /思源宋体/ })).toBeTruthy();
  });

  it('keeps TXT chapter recognition in the Books category', async () => {
    const onHeadingPatternChange = vi.fn();
    render(SettingsView, {
      settings: defaultAppSettings,
      theme: 'system',
      headingPattern: '^第.+章',
      headingPatternError: '表达式无效',
      onHeadingPatternChange,
    });

    await fireEvent.click(screen.getByRole('button', { name: '章节识别' }));
    await fireEvent.input(screen.getByLabelText('TXT 标题识别正则'), {
      target: { value: '[' },
    });

    expect(screen.getByRole('alert').textContent).toBe('表达式无效');
    expect(onHeadingPatternChange).toHaveBeenCalledWith('[');
  });
});
